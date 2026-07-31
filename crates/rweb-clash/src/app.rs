use crate::backup::BackupService;
use crate::controller::ControllerClient;
use crate::core::{CoreManager, CoreStartConfig};
use crate::egress::EgressProbe;
use crate::error::AppError;
use crate::instance_lock::{DataRootLock, GlobalAppLock};
use crate::manual::manual_node_record;
use crate::paths::AppPaths;
use crate::platform::{
    apply_system_proxy, begin_system_proxy_disable, complete_system_proxy_recovery,
    system_proxy_backup_exists, validate_tun_permissions, SystemProxyRestoreOutcome,
};
use crate::proxy::ProxyService;
use crate::rule::RuleService;
use crate::runtime::compile_runtime_yaml;
use crate::storage::Storage;
use crate::subscription::{cleanup_stale_subscription_candidates, SubscriptionSyncer};
use crate::types::{
    ConnectionResponse, CoreStatusResponse, DelayResponse, EgressResponse, FilterRuleInput,
    ManualNodeInput, ManualNodeResponse, OperationResponse, ProxyGroupRequest,
    ProxyTopologyResponse, RuleInput, RuleResponse, RuleSetInput, RuleSetResponse, RuleTestRequest,
    RuleTestResponse, SelectProxyRequest, SetupStatusResponse, SubscriptionInput,
    SubscriptionResponse, SystemConfig, SystemConfigPatch, SystemStatusResponse, TrafficResponse,
};
use crate::util::{new_id, parse_host_from_log, validate_url};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct App {
    inner: Arc<AppInner>,
}

#[derive(Debug)]
struct AppInner {
    paths: AppPaths,
    embedded_assets: Option<&'static crate::EmbeddedAssets>,
    storage: Storage,
    core: CoreManager,
    subscription_syncer: SubscriptionSyncer,
    proxy_service: ProxyService,
    rule_service: RuleService,
    egress_probe: EgressProbe,
    backup_service: BackupService,
    config_update: Mutex<()>,
    runtime_operation: Mutex<()>,
    rule_set_operation: Mutex<()>,
    background_started: OnceCell<()>,
    _data_root_lock: DataRootLock,
    _global_app_lock: GlobalAppLock,
}

#[derive(Debug, Clone)]
pub struct AppOptions {
    pub root_dir: Option<PathBuf>,
    pub packaged_resources: Option<PathBuf>,
    pub embedded_assets: Option<&'static crate::EmbeddedAssets>,
    pub listen_addr: SocketAddr,
}

#[derive(Debug)]
struct RuntimeActivationError {
    error: AppError,
    external_proxy_preserved: bool,
}

impl RuntimeActivationError {
    fn new(error: AppError, external_proxy_preserved: bool) -> Self {
        Self {
            error,
            external_proxy_preserved,
        }
    }
}

impl From<AppError> for RuntimeActivationError {
    fn from(error: AppError) -> Self {
        Self::new(error, false)
    }
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            root_dir: None,
            packaged_resources: None,
            embedded_assets: None,
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 31990)),
        }
    }
}

impl App {
    pub async fn initialize(options: AppOptions) -> Result<Self, AppError> {
        let paths = if let Some(root) = options.root_dir.as_ref() {
            AppPaths::from_root(root.clone())
        } else {
            AppPaths::discover()?
        };
        paths.ensure_dirs()?;
        let global_app_lock = GlobalAppLock::acquire(&paths)?;
        Self::initialize_with_global_lock(options, paths, global_app_lock).await
    }

    async fn initialize_with_global_lock(
        options: AppOptions,
        paths: AppPaths,
        global_app_lock: GlobalAppLock,
    ) -> Result<Self, AppError> {
        let data_root_lock = DataRootLock::acquire(&paths)?;
        cleanup_stale_subscription_candidates(&paths);
        info!(
            root_dir = %AppPaths::display(&paths.root_dir),
            data_dir = %AppPaths::display(&paths.data_dir),
            "initializing rweb-clash app"
        );
        let storage = Storage::connect(&paths).await?;
        let rule_service = RuleService::new(storage.clone(), paths.clone());
        match rule_service.cleanup_orphan_snapshots().await {
            Ok(report) if report.failed > 0 => warn!(
                removed = report.removed,
                failed = report.failed,
                "rule-set snapshot startup cleanup completed with errors"
            ),
            Ok(report) if report.removed > 0 => info!(
                removed = report.removed,
                "removed orphan rule-set snapshots during startup"
            ),
            Ok(_) => {}
            Err(error) => warn!(
                %error,
                "failed to run rule-set snapshot startup cleanup; a later startup will retry"
            ),
        }
        crate::bootstrap::bootstrap_runtime_assets(
            &paths,
            &storage,
            crate::bootstrap::BootstrapOptions {
                packaged_resources: options.packaged_resources.as_deref(),
                embedded_assets: options.embedded_assets,
            },
        )
        .await?;
        let core = CoreManager::new(paths.clone(), storage.clone());
        let backup_service = BackupService::new(storage.clone(), paths.clone());
        let app = Self {
            inner: Arc::new(AppInner {
                paths: paths.clone(),
                embedded_assets: options.embedded_assets,
                subscription_syncer: SubscriptionSyncer::new(storage.clone(), paths.clone()),
                proxy_service: ProxyService::new(storage.clone()),
                rule_service,
                egress_probe: EgressProbe::new(),
                backup_service,
                storage,
                core,
                config_update: Mutex::new(()),
                runtime_operation: Mutex::new(()),
                rule_set_operation: Mutex::new(()),
                background_started: OnceCell::new(),
                _data_root_lock: data_root_lock,
                _global_app_lock: global_app_lock,
            }),
        };

        let mut config = app.config().await?;
        let proxy_backup_found =
            system_proxy_backup_exists(&app.system_proxy_backup_path()).await?;
        if proxy_backup_found {
            let recovery =
                begin_system_proxy_disable(&config, &app.system_proxy_backup_path()).await;
            let (external_changes_preserved, recovery_failed, recovery_succeeded) = match recovery {
                Ok(Some(outcome)) => (outcome.external_changes_preserved, false, true),
                Ok(None) => (false, false, false),
                Err(error) => {
                    warn!(%error, "failed to recover system proxy backup during startup; watchdog will retry");
                    (false, true, false)
                }
            };
            if startup_recovery_disables_proxy_intent(external_changes_preserved, recovery_failed)
                && disable_system_proxy_intent(&mut config)
            {
                app.inner.storage.save_config(&config).await?;
                warn!(
                    external_changes_preserved,
                    recovery_failed,
                    "disabled persisted system proxy intent after interrupted proxy recovery"
                );
            }
            if recovery_succeeded {
                complete_system_proxy_recovery(&app.system_proxy_backup_path()).await?;
            }
        }
        if config.system_proxy && !config.auto_start {
            let mut disabled = config.clone();
            disabled.system_proxy = false;
            apply_system_proxy(&disabled, &app.system_proxy_backup_path()).await?;
            app.inner.storage.save_config(&disabled).await?;
            config = disabled;
            info!("disabled persisted system proxy because automatic core start is off");
        }
        info!(
            mode = %config.mode,
            mixed_port = config.mixed_port,
            tun = config.tun,
            system_proxy = config.system_proxy,
            auto_start = config.auto_start,
            "loaded system config"
        );
        {
            let _runtime_operation = app.inner.runtime_operation.lock().await;
            compile_runtime_yaml(&app.inner.storage, &app.inner.paths, &config).await?;
        }
        if config.auto_start {
            info!(
                auto_start = config.auto_start,
                system_proxy = config.system_proxy,
                "starting core for persisted runtime intent"
            );
            if let Err(err) = app.start_core().await {
                warn!(error = %err, "automatic core start failed");
                if config.system_proxy {
                    let mut disabled = config.clone();
                    disabled.system_proxy = false;
                    if let Err(cleanup_error) =
                        apply_system_proxy(&disabled, &app.system_proxy_backup_path()).await
                    {
                        warn!(%cleanup_error, "failed to restore system proxy after automatic core start failure");
                    } else if let Err(save_error) = app.inner.storage.save_config(&disabled).await {
                        warn!(%save_error, "failed to persist disabled system proxy after automatic core start failure");
                    }
                }
            }
        }
        app.start_background_tasks().await;
        Ok(app)
    }

    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
    }

    pub fn embedded_assets(&self) -> Option<&'static crate::EmbeddedAssets> {
        self.inner.embedded_assets
    }

    fn system_proxy_backup_path(&self) -> PathBuf {
        self.inner.paths.data_dir.join("system-proxy-backup.json")
    }

    pub async fn config(&self) -> Result<SystemConfig, AppError> {
        self.inner.storage.load_config().await
    }

    pub async fn update_config(&self, patch: SystemConfigPatch) -> Result<SystemConfig, AppError> {
        let _config_update = self.inner.config_update.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let current = self.config().await?;
        let mut config = current.clone();
        patch.apply(&mut config);
        validate_config(&config)?;
        if !current.tun && config.tun {
            validate_tun_permissions().await?;
        }
        info!(
            mode = %config.mode,
            mixed_port = config.mixed_port,
            tun = config.tun,
            system_proxy = config.system_proxy,
            external_controller_enabled = config.external_controller_enabled,
            "updating system config"
        );
        let core_was_running = self.inner.core.is_running().await;
        let requested_system_proxy = config.system_proxy;
        let config_persisted_early = requires_early_system_proxy_persist(&current, &config);
        if config_persisted_early {
            if let Err(error) = self.inner.storage.save_config(&config).await {
                return self
                    .config_update_failed(error, &current, &config, core_was_running, false)
                    .await;
            }
        }
        let needs_proxy_disable = current.system_proxy
            && (!config.system_proxy || current.mixed_port != config.mixed_port);
        let mut external_proxy_preserved = false;
        let mut activation_controller = current.clone();
        if needs_proxy_disable {
            let mut disabled = current.clone();
            disabled.system_proxy = false;
            let proxy_disable = if requested_system_proxy {
                self.begin_temporary_system_proxy_disable(&current, &disabled)
                    .await?
            } else {
                begin_system_proxy_disable(&disabled, &self.system_proxy_backup_path()).await?
            };
            external_proxy_preserved = proxy_disable_preserved_external(&proxy_disable);
            activation_controller.system_proxy = false;
            if external_proxy_preserved {
                config.system_proxy = false;
                self.inner.storage.save_config(&config).await?;
                if proxy_disable.is_some() {
                    complete_system_proxy_recovery(&self.system_proxy_backup_path()).await?;
                }
            }
        }
        let should_run = core_was_running || config.system_proxy;
        let proxy_changed = current.system_proxy != config.system_proxy
            || (config.system_proxy && current.mixed_port != config.mixed_port);
        let activation_external_proxy_preserved = match self
            .activate_runtime_config(&config, &activation_controller, should_run)
            .await
        {
            Ok(external_proxy_preserved) => external_proxy_preserved,
            Err(activation_error) => {
                return self
                    .config_update_failed(
                        activation_error.error,
                        &current,
                        &config,
                        core_was_running,
                        external_proxy_preserved || activation_error.external_proxy_preserved,
                    )
                    .await;
            }
        };
        external_proxy_preserved |= activation_external_proxy_preserved;
        if activation_external_proxy_preserved {
            config.system_proxy = false;
        }

        if proxy_changed {
            info!(
                system_proxy = config.system_proxy,
                mixed_port = config.mixed_port,
                "applying system proxy config"
            );
            if let Err(error) = apply_system_proxy(&config, &self.system_proxy_backup_path()).await
            {
                return self
                    .config_update_failed(
                        error,
                        &current,
                        &config,
                        core_was_running,
                        external_proxy_preserved,
                    )
                    .await;
            }
        }
        if !config_persisted_early {
            if let Err(error) = self.inner.storage.save_config(&config).await {
                return self
                    .config_update_failed(
                        error,
                        &current,
                        &config,
                        core_was_running,
                        external_proxy_preserved,
                    )
                    .await;
            }
        }
        Ok(config)
    }

    pub async fn system_status(&self) -> Result<SystemStatusResponse, AppError> {
        let config = self.config().await?;
        let core = self.core_status().await?;
        Ok(SystemStatusResponse { core, config })
    }

    pub async fn setup_status(&self) -> Result<SetupStatusResponse, AppError> {
        let config = self.config().await?;
        let subscriptions = self.list_subscriptions().await?;
        let manual_node_count = self.manual_nodes().await?.len();
        let has_sources = !subscriptions.is_empty() || manual_node_count > 0;
        let core_path = self.inner.paths.mihomo_binary();
        let core_ready = core_path.is_file();
        let mixed_port_available = port_available(config.mixed_port).await;
        let controller_port_available = controller_port_available(&config).await;
        let mut warnings = Vec::new();
        if !core_ready {
            warnings.push(
                "Mihomo core missing. Reinstall or rebuild the package with core resources.".into(),
            );
        }
        if !mixed_port_available {
            warnings.push(format!(
                "Mixed proxy port {} is already in use.",
                config.mixed_port
            ));
        }
        if !controller_port_available {
            warnings.push(format!(
                "Controller address {} is already in use.",
                config.external_controller
            ));
        }

        Ok(SetupStatusResponse {
            needs_onboarding: !has_sources || !core_ready,
            has_subscriptions: !subscriptions.is_empty(),
            subscription_count: subscriptions.len(),
            has_sources,
            manual_node_count,
            core_ready,
            core_path: AppPaths::display(&core_path),
            mixed_port_available,
            controller_port_available,
            warnings,
        })
    }

    pub async fn egress(&self) -> Result<EgressResponse, AppError> {
        let config = self.config().await?;
        let core = self
            .inner
            .core
            .snapshot(config.external_controller.clone())
            .await;
        let proxy_url = egress_proxy_url(&core.state, config.mixed_port);
        self.inner.egress_probe.probe(proxy_url.as_deref()).await
    }

    pub async fn core_status(&self) -> Result<CoreStatusResponse, AppError> {
        let config = self.config().await?;
        Ok(self
            .inner
            .core
            .snapshot(config.external_controller.clone())
            .await)
    }

    pub async fn start_core(&self) -> Result<CoreStatusResponse, AppError> {
        let _config_update = self.inner.config_update.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let config = self.config().await?;
        if config.tun {
            validate_tun_permissions().await?;
        }
        if !self.inner.core.is_running().await {
            ensure_runtime_ports_available(&config).await?;
        }
        let runtime = compile_runtime_yaml(&self.inner.storage, &self.inner.paths, &config).await?;
        info!(
            controller_addr = %config.external_controller,
            runtime_yaml = %AppPaths::display(&runtime),
            "starting core"
        );
        let status = self
            .inner
            .core
            .start(self.core_start_config(&config, runtime))
            .await?;
        if config.system_proxy {
            if let Err(error) = apply_system_proxy(&config, &self.system_proxy_backup_path()).await
            {
                let mut disabled = config;
                disabled.system_proxy = false;
                let cleanup_result =
                    apply_system_proxy(&disabled, &self.system_proxy_backup_path()).await;
                let save_result = self.inner.storage.save_config(&disabled).await;
                return match (cleanup_result, save_result) {
                    (Ok(()), Ok(())) => Err(error),
                    (cleanup, save) => Err(AppError::internal(format!(
                        "enabling system proxy failed ({error}); cleanup result: {cleanup:?}; persistence result: {save:?}"
                    ))),
                };
            }
        }
        self.synchronize_proxy_selections(&config, true).await?;
        Ok(status)
    }

    pub async fn stop_core(&self) -> Result<CoreStatusResponse, AppError> {
        let _config_update = self.inner.config_update.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let config = self.config().await?;
        info!(
            controller_addr = %config.external_controller,
            "stopping core"
        );
        let mut disabled = config.clone();
        disabled.system_proxy = false;
        disabled.auto_start = false;
        let proxy_disable = if config.system_proxy {
            self.inner.storage.save_config(&disabled).await?;
            begin_system_proxy_disable(&disabled, &self.system_proxy_backup_path()).await?
        } else {
            None
        };
        let external_proxy_preserved = proxy_disable_preserved_external(&proxy_disable);
        let status = match self
            .inner
            .core
            .stop(config.external_controller.clone())
            .await
        {
            Ok(status) => status,
            Err(error) => {
                if config.system_proxy {
                    if external_proxy_preserved {
                        if proxy_disable.is_some() {
                            complete_system_proxy_recovery(&self.system_proxy_backup_path())
                                .await?;
                        }
                    } else if let Err(rollback_error) = self
                        .restore_proxy_intent_after_failed_stop(&config, &disabled)
                        .await
                    {
                        return Err(AppError::internal(format!(
                            "stopping core failed ({error}); restoring the previous proxy intent failed ({rollback_error})"
                        )));
                    }
                }
                return Err(error);
            }
        };
        if proxy_disable.is_some() {
            complete_system_proxy_recovery(&self.system_proxy_backup_path()).await?;
        }
        if config.auto_start && !config.system_proxy {
            self.inner.storage.save_config(&disabled).await?;
        }
        Ok(status)
    }

    pub async fn restart_core(&self) -> Result<CoreStatusResponse, AppError> {
        let _config_update = self.inner.config_update.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let config = self.config().await?;
        if config.tun {
            validate_tun_permissions().await?;
        }
        let runtime = compile_runtime_yaml(&self.inner.storage, &self.inner.paths, &config).await?;
        info!(
            controller_addr = %config.external_controller,
            runtime_yaml = %AppPaths::display(&runtime),
            "restarting core"
        );
        let mut disabled = config.clone();
        disabled.system_proxy = false;
        let proxy_disable = if config.system_proxy {
            self.begin_temporary_system_proxy_disable(&config, &disabled)
                .await?
        } else {
            None
        };
        let external_proxy_preserved = proxy_disable_preserved_external(&proxy_disable);
        if external_proxy_preserved {
            self.inner.storage.save_config(&disabled).await?;
        }
        let status = self
            .inner
            .core
            .restart(self.core_start_config(&config, runtime))
            .await;
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                if config.system_proxy {
                    if !external_proxy_preserved {
                        self.inner.storage.save_config(&disabled).await?;
                    }
                    if proxy_disable.is_some() {
                        complete_system_proxy_recovery(&self.system_proxy_backup_path()).await?;
                    }
                }
                return Err(error);
            }
        };
        if config.system_proxy && !external_proxy_preserved {
            if let Err(error) = apply_system_proxy(&config, &self.system_proxy_backup_path()).await
            {
                let save_result = self.inner.storage.save_config(&disabled).await;
                let cleanup_result =
                    apply_system_proxy(&disabled, &self.system_proxy_backup_path()).await;
                return match (cleanup_result, save_result) {
                    (Ok(()), Ok(())) => Err(error),
                    (cleanup, save) => Err(AppError::internal(format!(
                        "re-enabling system proxy after restart failed ({error}); cleanup result: {cleanup:?}; persistence result: {save:?}"
                    ))),
                };
            }
        } else if proxy_disable.is_some() {
            complete_system_proxy_recovery(&self.system_proxy_backup_path()).await?;
        }
        self.synchronize_proxy_selections(&config, true).await?;
        Ok(status)
    }

    pub async fn list_subscriptions(&self) -> Result<Vec<SubscriptionResponse>, AppError> {
        self.inner.storage.list_subscriptions().await
    }

    pub async fn subscription_members(
        &self,
        id: &str,
    ) -> Result<crate::types::SubscriptionMembersResponse, AppError> {
        self.inner.storage.subscription_members(id).await
    }

    pub async fn create_subscription(
        &self,
        input: SubscriptionInput,
    ) -> Result<Vec<SubscriptionResponse>, AppError> {
        validate_subscription_input(&input)?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        let id = new_id("sub");
        info!(
            subscription_id = %id,
            name = %input.name.trim(),
            interval_seconds = input.interval_seconds(),
            inherit_global = input.inherit_global.unwrap_or(true),
            rules = input.rules.len(),
            "creating subscription"
        );
        self.inner
            .storage
            .create_pending_subscription_with_route(
                &id,
                input.name.trim(),
                input.url.trim(),
                input.interval_seconds(),
                input.inherit_global.unwrap_or(true),
                &input.rules,
                input.download_route,
            )
            .await?;
        if let Err(err) = self
            .inner
            .subscription_syncer
            .refresh(&id, core_was_running)
            .await
        {
            return match self.inner.storage.delete_subscription(&id).await {
                Ok(()) => Err(err),
                Err(rollback_error) => Err(AppError::internal(format!(
                    "initial subscription refresh failed ({err}); deleting the new subscription failed ({rollback_error})"
                ))),
            };
        }
        if let Err(error) = self.refresh_runtime_locked(core_was_running).await {
            let delete_result = self.inner.storage.delete_subscription(&id).await;
            let runtime_restore = if delete_result.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (delete_result, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (delete_result, runtime_restore) => Err(AppError::internal(format!(
                    "activating a new subscription failed ({error}); deleting it: {delete_result:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        if let Err(error) = self.inner.storage.activate_subscription(&id).await {
            let delete_result = self.inner.storage.delete_subscription(&id).await;
            let runtime_restore = if delete_result.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (delete_result, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (delete_result, runtime_restore) => Err(AppError::internal(format!(
                    "finalizing a new subscription failed ({error}); deleting it: {delete_result:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        self.list_subscriptions().await
    }

    pub async fn update_subscription(
        &self,
        id: &str,
        input: SubscriptionInput,
    ) -> Result<Vec<SubscriptionResponse>, AppError> {
        validate_subscription_input(&input)?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        let refresh_guard = self.inner.subscription_syncer.lock_refresh(id).await;
        let previous = self
            .list_subscriptions()
            .await?
            .into_iter()
            .find(|subscription| subscription.id == id)
            .ok_or_else(|| {
                AppError::not_found(
                    "subscription_not_found",
                    format!("subscription {id} not found"),
                )
            })?;
        info!(
            subscription_id = %id,
            name = %input.name.trim(),
            interval_seconds = input.interval_seconds(),
            inherit_global = input.inherit_global.unwrap_or(true),
            rules = input.rules.len(),
            "updating subscription"
        );
        self.inner
            .storage
            .update_subscription_with_route(
                id,
                input.name.trim(),
                input.url.trim(),
                input.interval_seconds(),
                input.inherit_global.unwrap_or(true),
                &input.rules,
                input.download_route,
            )
            .await?;
        if let Err(err) = self
            .inner
            .subscription_syncer
            .refresh_locked(id, core_was_running)
            .await
        {
            let previous_rules = previous
                .rules
                .iter()
                .map(|rule| FilterRuleInput {
                    id: Some(rule.id.clone()),
                    action: rule.action.clone(),
                    match_type: rule.match_type.clone(),
                    pattern: rule.pattern.clone(),
                    values: rule.values.clone(),
                    enabled: Some(rule.enabled),
                })
                .collect::<Vec<_>>();
            if let Err(rollback_error) = self
                .inner
                .storage
                .update_subscription_with_route(
                    id,
                    &previous.name,
                    &previous.url,
                    previous.interval_seconds.max(0) as u64,
                    previous.inherit_global,
                    &previous_rules,
                    previous.download_route,
                )
                .await
            {
                return Err(AppError::internal(format!(
                    "subscription refresh failed ({err}); restoring the previous subscription settings failed ({rollback_error})"
                )));
            }
            return Err(err);
        }
        drop(refresh_guard);
        self.refresh_runtime_locked(core_was_running).await?;
        self.list_subscriptions().await
    }

    pub async fn delete_subscription(&self, id: &str) -> Result<(), AppError> {
        info!(subscription_id = %id, "deleting subscription");
        self.inner.storage.get_subscription_url(id).await?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        let _refresh_guard = self.inner.subscription_syncer.lock_refresh(id).await;
        self.inner.storage.stage_subscription_deletion(id).await?;
        if let Err(error) = self.refresh_runtime_locked(core_was_running).await {
            let metadata_restore = self.inner.storage.restore_subscription_deletion(id).await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "deactivating subscription {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        if let Err(error) = self.inner.storage.delete_subscription(id).await {
            let metadata_restore = self.inner.storage.restore_subscription_deletion(id).await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "committing subscription deletion {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        Ok(())
    }

    pub async fn refresh_subscription(&self, id: &str) -> Result<(), AppError> {
        info!(subscription_id = %id, "refreshing subscription");
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        self.inner
            .subscription_syncer
            .refresh(id, core_was_running)
            .await?;
        self.refresh_runtime_locked(core_was_running).await?;
        Ok(())
    }

    pub async fn global_filter_rules(&self) -> Result<Vec<crate::types::FilterRule>, AppError> {
        self.inner.storage.list_global_filter_rules().await
    }

    pub async fn replace_global_filter_rules(
        &self,
        rules: Vec<FilterRuleInput>,
    ) -> Result<Vec<crate::types::FilterRule>, AppError> {
        for rule in &rules {
            validate_filter_rule_input(rule)?;
        }
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            rules = rules.len(),
            "replacing global subscription filter rules"
        );
        let rules = self
            .inner
            .storage
            .replace_global_filter_rules(&rules)
            .await?;
        let subscription_ids = self
            .inner
            .storage
            .subscription_ids_inheriting_global_rules()
            .await?;
        let mut refresh_failures = Vec::new();
        for id in subscription_ids {
            if let Err(error) = self
                .inner
                .subscription_syncer
                .refresh(&id, core_was_running)
                .await
            {
                warn!(
                    subscription_id = %id,
                    error = %error,
                    "subscription refresh after global filter update failed"
                );
                refresh_failures.push((id, error));
            }
        }
        let runtime_result = self.refresh_runtime_locked(core_was_running).await;
        if let Some((first_id, mut first_error)) = refresh_failures.into_iter().next() {
            first_error.message = format!(
                "global filters were saved, but refreshing inherited subscription {first_id} failed: {}",
                first_error.message
            );
            return Err(first_error);
        }
        runtime_result?;
        Ok(rules)
    }

    pub async fn proxy_topology(&self) -> Result<ProxyTopologyResponse, AppError> {
        let (groups, nodes) = self.inner.storage.proxy_topology().await?;
        Ok(ProxyTopologyResponse { groups, nodes })
    }

    pub async fn manual_nodes(&self) -> Result<Vec<ManualNodeResponse>, AppError> {
        self.inner.storage.list_manual_nodes().await
    }

    pub async fn create_manual_node(
        &self,
        input: ManualNodeInput,
    ) -> Result<Vec<ManualNodeResponse>, AppError> {
        let item = manual_node_record(input)?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        self.inner.storage.create_manual_node(&item).await?;
        self.inner.storage.sync_builtin_proxy_group().await?;
        self.refresh_runtime_locked(core_was_running).await?;
        self.manual_nodes().await
    }

    pub async fn update_manual_node(
        &self,
        name: &str,
        mut input: ManualNodeInput,
    ) -> Result<Vec<ManualNodeResponse>, AppError> {
        input.name = name.to_string();
        let item = manual_node_record(input)?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        self.inner.storage.update_manual_node(&item).await?;
        self.refresh_runtime_locked(core_was_running).await?;
        self.manual_nodes().await
    }

    pub async fn delete_manual_node(&self, name: &str) -> Result<(), AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        self.inner.storage.delete_manual_node(name).await?;
        self.inner.storage.sync_builtin_proxy_group().await?;
        self.refresh_runtime_locked(core_was_running).await
    }

    pub async fn create_proxy_group(&self, input: ProxyGroupRequest) -> Result<(), AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            name = %input.name.trim(),
            group_type = %input.group_type,
            filters = input.filter.len(),
            "creating proxy group"
        );
        self.inner.proxy_service.create_group(input).await?;
        self.refresh_runtime_locked(core_was_running).await
    }

    pub async fn update_proxy_group(
        &self,
        group: &str,
        input: ProxyGroupRequest,
    ) -> Result<(), AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            group = %group,
            name = %input.name.trim(),
            group_type = %input.group_type,
            filters = input.filter.len(),
            "updating proxy group"
        );
        self.inner.proxy_service.update_group(group, input).await?;
        self.refresh_runtime_locked(core_was_running).await
    }

    pub async fn delete_proxy_group(&self, group: &str) -> Result<(), AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(group = %group, "deleting proxy group");
        self.inner.storage.delete_custom_group(group).await?;
        self.refresh_runtime_locked(core_was_running).await
    }

    pub async fn select_proxy(
        &self,
        group: &str,
        request: SelectProxyRequest,
    ) -> Result<OperationResponse, AppError> {
        info!(group = %group, name = %request.name, "selecting proxy");
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let controller = if self.inner.core.is_running().await {
            Some(self.controller_client().await?)
        } else {
            None
        };
        let previous = self
            .inner
            .storage
            .set_group_now(group, &request.name)
            .await?;
        if let Some(controller) = controller {
            if let Err(error) = controller.select_proxy(group, &request.name).await {
                let controller_rollback = match previous.as_deref() {
                    Some(previous) => controller.select_proxy(group, previous).await,
                    None => Ok(()),
                };
                let storage_rollback = self
                    .inner
                    .storage
                    .restore_group_now(group, previous.as_deref())
                    .await;
                return match (controller_rollback, storage_rollback) {
                    (Ok(()), Ok(())) => Err(error),
                    (controller_rollback, storage_rollback) => Err(AppError::internal(format!(
                        "proxy selection failed ({error}); controller rollback: {controller_rollback:?}; persistence rollback: {storage_rollback:?}"
                    ))),
                };
            }
        }
        Ok(OperationResponse::ok("proxy updated"))
    }

    pub async fn test_node(&self, name: &str) -> Result<DelayResponse, AppError> {
        if !self.inner.core.is_running().await {
            return Ok(DelayResponse {
                name: name.to_string(),
                delay: 0,
            });
        }
        let config = self.config().await?;
        let controller = self.controller_client().await?;
        let result = controller
            .proxy_delay(name, &config.delay_test_url, config.delay_test_timeout_ms)
            .await?;
        self.inner
            .storage
            .set_node_delay(name, result.delay)
            .await?;
        Ok(result)
    }

    pub async fn test_group(&self, name: &str) -> Result<Vec<DelayResponse>, AppError> {
        if !self.inner.core.is_running().await {
            return Ok(Vec::new());
        }
        let config = self.config().await?;
        let controller = self.controller_client().await?;
        let result = controller
            .group_delay(name, &config.delay_test_url, config.delay_test_timeout_ms)
            .await?;
        if let Some(best) = result
            .iter()
            .filter(|item| item.delay > 0)
            .min_by_key(|item| item.delay)
        {
            self.inner.storage.set_group_delay(name, best.delay).await?;
        }
        for item in &result {
            self.inner
                .storage
                .set_node_delay(&item.name, item.delay)
                .await?;
        }
        Ok(result)
    }

    pub async fn list_rules(&self) -> Result<Vec<RuleResponse>, AppError> {
        self.inner.storage.list_rules().await
    }

    pub async fn create_rule(&self, input: RuleInput) -> Result<RuleResponse, AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            rule_type = %input.rule_type,
            policy = %input.policy,
            enabled = input.enabled.unwrap_or(true),
            "creating routing rule"
        );
        let rule = self.inner.rule_service.create_rule(input).await?;
        self.refresh_runtime_locked(core_was_running).await?;
        Ok(rule)
    }

    pub async fn update_rule(&self, id: &str, input: RuleInput) -> Result<RuleResponse, AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            rule_id = %id,
            rule_type = %input.rule_type,
            policy = %input.policy,
            enabled = input.enabled.unwrap_or(true),
            "updating routing rule"
        );
        let rule = self.inner.rule_service.update_rule(id, input).await?;
        self.refresh_runtime_locked(core_was_running).await?;
        Ok(rule)
    }

    pub async fn delete_rule(&self, id: &str) -> Result<(), AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(rule_id = %id, "deleting routing rule");
        self.inner.storage.delete_rule(id).await?;
        self.refresh_runtime_locked(core_was_running).await
    }

    pub async fn test_rule(&self, input: RuleTestRequest) -> Result<RuleTestResponse, AppError> {
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        self.inner.rule_service.test_rule(input).await
    }

    pub async fn list_rule_sets(&self) -> Result<Vec<RuleSetResponse>, AppError> {
        self.inner.storage.list_rule_sets().await
    }

    pub async fn create_rule_set(&self, input: RuleSetInput) -> Result<RuleSetResponse, AppError> {
        let _rule_set_operation = self.inner.rule_set_operation.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(
            name = %input.name.trim(),
            format = %input.format.as_deref().unwrap_or("text"),
            interval_seconds = input.interval_seconds(),
            "creating rule set"
        );
        let rule_set = self.inner.rule_service.create_rule_set(input).await?;
        if let Err(error) = self.refresh_runtime_locked(core_was_running).await {
            let delete_result = self.inner.rule_service.delete_rule_set(&rule_set.id).await;
            let runtime_restore = if delete_result.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (delete_result, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (delete_result, runtime_restore) => Err(AppError::internal(format!(
                    "activating a new rule set failed ({error}); deleting it: {delete_result:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        if let Err(error) = self.inner.storage.activate_rule_set(&rule_set.id).await {
            let delete_result = self.inner.rule_service.delete_rule_set(&rule_set.id).await;
            let runtime_restore = if delete_result.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            self.cleanup_rule_set_snapshots().await;
            return match (delete_result, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (delete_result, runtime_restore) => Err(AppError::internal(format!(
                    "finalizing a new rule set failed ({error}); deleting it: {delete_result:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        self.cleanup_rule_set_snapshots().await;
        Ok(rule_set)
    }

    pub async fn refresh_rule_set(&self, id: &str) -> Result<(), AppError> {
        let _rule_set_operation = self.inner.rule_set_operation.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(rule_set_id = %id, "refreshing rule set");
        let previous = self.inner.rule_service.refresh_rule_set(id).await?;
        if let Err(error) = self.refresh_runtime_locked(core_was_running).await {
            let metadata_restore = self
                .inner
                .rule_service
                .restore_rule_set_refresh(id, &previous)
                .await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            if metadata_restore.is_ok() {
                if let Err(mark_error) = self
                    .inner
                    .storage
                    .mark_rule_set_refresh_error(id, &error.message)
                    .await
                {
                    warn!(rule_set_id = %id, %mark_error, "failed to persist rule-set activation error");
                }
            }
            self.cleanup_rule_set_snapshots().await;
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "activating refreshed rule set {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        if let Err(error) = self.inner.storage.activate_rule_set(id).await {
            let metadata_restore = self
                .inner
                .rule_service
                .restore_rule_set_refresh(id, &previous)
                .await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            if metadata_restore.is_ok() {
                if let Err(mark_error) = self
                    .inner
                    .storage
                    .mark_rule_set_refresh_error(id, &error.message)
                    .await
                {
                    warn!(rule_set_id = %id, %mark_error, "failed to persist rule-set commit error");
                }
            }
            self.cleanup_rule_set_snapshots().await;
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "committing refreshed rule set {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        self.cleanup_rule_set_snapshots().await;
        Ok(())
    }

    pub async fn delete_rule_set(&self, id: &str) -> Result<(), AppError> {
        let _rule_set_operation = self.inner.rule_set_operation.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let core_was_running = self.inner.core.is_running().await;
        info!(rule_set_id = %id, "deleting rule set");
        self.inner.storage.stage_rule_set_deletion(id).await?;
        if let Err(error) = self.refresh_runtime_locked(core_was_running).await {
            let metadata_restore = self.inner.storage.restore_rule_set_deletion(id).await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "deactivating rule set {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        if let Err(error) = self.inner.rule_service.delete_rule_set(id).await {
            let metadata_restore = self.inner.storage.restore_rule_set_deletion(id).await;
            let runtime_restore = if metadata_restore.is_ok() {
                self.refresh_runtime_locked(core_was_running).await
            } else {
                Ok(())
            };
            return match (metadata_restore, runtime_restore) {
                (Ok(()), Ok(())) => Err(error),
                (metadata_restore, runtime_restore) => Err(AppError::internal(format!(
                    "committing rule-set deletion {id} failed ({error}); restoring metadata: {metadata_restore:?}; restoring runtime: {runtime_restore:?}"
                ))),
            };
        }
        self.cleanup_rule_set_snapshots().await;
        Ok(())
    }

    async fn cleanup_rule_set_snapshots(&self) {
        if let Err(error) = self.inner.rule_service.cleanup_orphan_snapshots().await {
            warn!(%error, "failed to clean obsolete rule-set snapshots");
        }
    }

    pub async fn logs(
        &self,
        level: Option<&str>,
        search: Option<&str>,
    ) -> Result<Vec<crate::types::LogEntryResponse>, AppError> {
        self.inner.storage.list_logs(level, search, 1000).await
    }

    pub async fn clear_logs(&self) -> Result<(), AppError> {
        info!("clearing logs");
        self.inner.storage.clear_logs().await
    }

    pub async fn export_logs(&self) -> Result<String, AppError> {
        self.inner.storage.log_export_text().await
    }

    pub async fn export_diagnostics(&self) -> Result<String, AppError> {
        let setup = self.setup_status().await?;
        let system = self.system_status().await?;
        let logs = self.export_logs().await.unwrap_or_default();
        let mut output = String::new();
        output.push_str("# rweb-clash diagnostics\n\n");
        output.push_str("## Paths\n");
        output.push_str(&format!(
            "- root_dir: {}\n- data_dir: {}\n- runtime_yaml: {}\n- mihomo_binary: {}\n\n",
            AppPaths::display(&self.inner.paths.root_dir),
            AppPaths::display(&self.inner.paths.data_dir),
            AppPaths::display(&self.inner.paths.runtime_yaml),
            setup.core_path,
        ));
        output.push_str("## Setup\n");
        output.push_str(&format!(
            "- needs_onboarding: {}\n- core_ready: {}\n- subscriptions: {}\n- mixed_port_available: {}\n- controller_port_available: {}\n",
            setup.needs_onboarding,
            setup.core_ready,
            setup.subscription_count,
            setup.mixed_port_available,
            setup.controller_port_available,
        ));
        if !setup.warnings.is_empty() {
            output.push_str("- warnings:\n");
            for warning in setup.warnings {
                output.push_str(&format!("  - {warning}\n"));
            }
        }
        output.push_str("\n## Runtime\n");
        output.push_str(&format!(
            "- core_state: {}\n- pid: {:?}\n- controller: {}\n- mode: {}\n- system_proxy: {}\n- tun: {}\n- mixed_port: {}\n",
            system.core.state,
            system.core.pid,
            system.core.controller_addr,
            system.config.mode,
            system.config.system_proxy,
            system.config.tun,
            system.config.mixed_port,
        ));
        if let Some(error) = system.core.last_error {
            output.push_str(&format!("- last_error: {error}\n"));
        }
        output.push_str("\n## Recent Logs\n");
        output.push_str(&logs);
        Ok(output)
    }

    pub async fn webdav_settings(&self) -> Result<crate::types::WebDavSettingsResponse, AppError> {
        self.inner.backup_service.settings().await
    }

    pub async fn save_webdav_settings(
        &self,
        input: crate::types::WebDavSettingsInput,
    ) -> Result<crate::types::WebDavSettingsResponse, AppError> {
        self.inner.backup_service.save_settings(input).await
    }

    pub async fn test_webdav(&self) -> Result<(), AppError> {
        self.inner.backup_service.test_webdav().await
    }

    pub async fn backups(&self) -> Result<Vec<crate::types::BackupResponse>, AppError> {
        self.inner.backup_service.list_backups().await
    }

    pub async fn create_backup(&self) -> Result<crate::types::BackupResponse, AppError> {
        self.inner.backup_service.create_backup().await
    }

    pub async fn delete_backup(&self, name: &str) -> Result<(), AppError> {
        self.inner.backup_service.delete_backup(name).await
    }

    pub async fn sync_webdav(&self) -> Result<crate::types::BackupResponse, AppError> {
        self.inner.backup_service.sync_to_webdav().await
    }

    pub async fn restore_backup(&self, name: &str) -> Result<(), AppError> {
        self.stop_core().await?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        self.inner.backup_service.restore_local(name).await?;
        self.compile_restored_runtime().await
    }

    pub async fn restore_webdav(&self) -> Result<(), AppError> {
        let _safety_backup = self.inner.backup_service.create_backup().await?;
        self.stop_core().await?;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        self.inner.backup_service.restore_webdav().await?;
        self.compile_restored_runtime().await
    }

    async fn compile_restored_runtime(&self) -> Result<(), AppError> {
        let mut config = self.config().await?;
        config.system_proxy = false;
        validate_config(&config)?;
        self.inner.storage.save_config(&config).await?;
        compile_runtime_yaml(&self.inner.storage, &self.inner.paths, &config).await?;
        Ok(())
    }

    pub async fn traffic(&self) -> TrafficResponse {
        if !self.inner.core.is_running().await {
            return TrafficResponse { up: 0, down: 0 };
        }
        match self.controller_client().await {
            Ok(controller) => controller
                .traffic_sample()
                .await
                .unwrap_or(TrafficResponse { up: 0, down: 0 }),
            Err(_) => TrafficResponse { up: 0, down: 0 },
        }
    }

    pub async fn connections(&self) -> Vec<ConnectionResponse> {
        if !self.inner.core.is_running().await {
            return Vec::new();
        }
        match self.controller_client().await {
            Ok(controller) => controller.connections().await.unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub async fn close_connection(&self, id: &str) -> Result<(), AppError> {
        let controller = self.controller_client().await?;
        controller.close_connection(id).await
    }

    pub async fn close_all_connections(&self) -> Result<(), AppError> {
        if !self.inner.core.is_running().await {
            return Ok(());
        }
        let controller = self.controller_client().await?;
        controller.close_all_connections().await
    }

    pub async fn flush_dns(&self) -> Result<(), AppError> {
        if self.inner.core.is_running().await {
            self.flush_controller_dns().await?;
        }
        Ok(())
    }

    async fn flush_controller_dns(&self) -> Result<(), AppError> {
        let controller = self.controller_client().await?;
        controller.flush_dns().await
    }

    pub async fn shutdown(&self) -> Result<(), AppError> {
        info!("shutting down rweb-clash app");
        let _config_update = self.inner.config_update.lock().await;
        let _runtime_operation = self.inner.runtime_operation.lock().await;
        let config = self.config().await?;
        let backup_found = system_proxy_backup_exists(&self.system_proxy_backup_path()).await?;
        let proxy_disable = if system_proxy_recovery_required(&config, backup_found) {
            let mut disabled = config.clone();
            disabled.system_proxy = false;
            let proxy_disable = if config.system_proxy {
                self.begin_temporary_system_proxy_disable(&config, &disabled)
                    .await?
            } else {
                begin_system_proxy_disable(&disabled, &self.system_proxy_backup_path()).await?
            };
            if proxy_disable_preserved_external(&proxy_disable) {
                self.inner.storage.save_config(&disabled).await?;
            }
            proxy_disable
        } else {
            None
        };
        self.inner.core.stop(config.external_controller).await?;
        if proxy_disable.is_some() {
            complete_system_proxy_recovery(&self.system_proxy_backup_path()).await?;
        }
        Ok(())
    }

    async fn controller_client(&self) -> Result<ControllerClient, AppError> {
        let config = self.config().await?;
        ControllerClient::new(config.external_controller, Some(config.secret))
    }

    async fn synchronize_proxy_selections(
        &self,
        config: &SystemConfig,
        core_was_restarted: bool,
    ) -> Result<(), AppError> {
        if !config.store_selected {
            if core_was_restarted {
                self.inner.storage.clear_group_selections().await?;
            }
            return Ok(());
        }
        let controller = ControllerClient::new(
            config.external_controller.clone(),
            Some(config.secret.clone()),
        )?;
        let (groups, _) = self.inner.storage.proxy_topology().await?;
        for group in groups
            .into_iter()
            .filter(|group| group.group_type == "select")
        {
            let Some(selected) = group.now else {
                continue;
            };
            controller
                .select_proxy(&group.name, &selected)
                .await
                .map_err(|mut error| {
                    error.message = format!(
                        "failed to replay persisted selection {selected} for group {}: {}",
                        group.name, error.message
                    );
                    error
                })?;
        }
        Ok(())
    }

    fn core_start_config(&self, config: &SystemConfig, runtime_yaml: PathBuf) -> CoreStartConfig {
        CoreStartConfig {
            controller_addr: config.external_controller.clone(),
            controller_secret: config.secret.clone(),
            controller_enabled: config.external_controller_enabled,
            mihomo_binary: self.inner.paths.mihomo_binary(),
            runtime_yaml,
            runtime_dir: self.inner.paths.profiles_dir.clone(),
            log_level: config.log_level.clone(),
            tun: config.tun,
        }
    }

    async fn refresh_runtime_locked(&self, should_run: bool) -> Result<(), AppError> {
        let config = self.config().await?;
        self.activate_runtime_config(&config, &config, should_run)
            .await
            .map(|_| ())
            .map_err(|error| error.error)
    }

    async fn activate_runtime_config(
        &self,
        config: &SystemConfig,
        controller_config: &SystemConfig,
        should_run: bool,
    ) -> Result<bool, RuntimeActivationError> {
        info!("compiling runtime config");
        let path = compile_runtime_yaml(&self.inner.storage, &self.inner.paths, config).await?;
        if !should_run {
            let mut external_proxy_preserved = false;
            if self.inner.core.is_running().await {
                if controller_config.system_proxy {
                    let mut disabled = controller_config.clone();
                    disabled.system_proxy = false;
                    let proxy_disable =
                        begin_system_proxy_disable(&disabled, &self.system_proxy_backup_path())
                            .await?;
                    external_proxy_preserved = proxy_disable_preserved_external(&proxy_disable);
                    if external_proxy_preserved {
                        self.inner
                            .storage
                            .save_config(&disabled)
                            .await
                            .map_err(|error| RuntimeActivationError::new(error, true))?;
                    }
                    if proxy_disable.is_some() {
                        complete_system_proxy_recovery(&self.system_proxy_backup_path())
                            .await
                            .map_err(|error| {
                                RuntimeActivationError::new(error, external_proxy_preserved)
                            })?;
                    }
                }
                self.inner
                    .core
                    .stop(controller_config.external_controller.clone())
                    .await
                    .map_err(|error| {
                        RuntimeActivationError::new(error, external_proxy_preserved)
                    })?;
            }
            info!(
                runtime_yaml = %AppPaths::display(&path),
                "runtime compiled, core is not running"
            );
            return Ok(external_proxy_preserved);
        }

        if self.inner.core.is_running().await {
            let controller = ControllerClient::new(
                controller_config.external_controller.clone(),
                Some(controller_config.secret.clone()),
            )?;
            info!(
                runtime_yaml = %AppPaths::display(&path),
                "reloading core runtime config"
            );
            let requires_restart = runtime_change_requires_restart(config, controller_config);
            if !requires_restart
                && controller
                    .reload_config(&AppPaths::display(&path))
                    .await
                    .is_ok()
            {
                self.synchronize_proxy_selections(config, false).await?;
                return Ok(false);
            }
            if requires_restart {
                info!("TUN mode requires a mihomo restart");
            } else {
                warn!("controller reload failed, restarting mihomo");
            }
            if config.tun {
                validate_tun_permissions().await?;
            }
            let mut proxy_disable = None;
            let mut external_proxy_preserved = false;
            let mut disabled = controller_config.clone();
            disabled.system_proxy = false;
            if controller_config.system_proxy {
                proxy_disable = match begin_system_proxy_disable(
                    &disabled,
                    &self.system_proxy_backup_path(),
                )
                .await
                {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        if let Err(rollback_error) =
                            apply_system_proxy(controller_config, &self.system_proxy_backup_path())
                                .await
                        {
                            let safety_persistence =
                                self.inner.storage.save_config(&disabled).await;
                            return Err(RuntimeActivationError::new(
                                AppError::internal(format!(
                                    "disabling system proxy failed ({error}); re-enabling it failed ({rollback_error}); disabled safety intent: {safety_persistence:?}"
                                )),
                                true,
                            ));
                        }
                        return Err(RuntimeActivationError::new(error, false));
                    }
                };
                external_proxy_preserved = proxy_disable_preserved_external(&proxy_disable);
                if external_proxy_preserved {
                    self.inner
                        .storage
                        .save_config(&disabled)
                        .await
                        .map_err(|error| RuntimeActivationError::new(error, true))?;
                }
            }
            let restart = self
                .inner
                .core
                .restart(self.core_start_config(config, path))
                .await;
            if let Err(error) = restart {
                if controller_config.system_proxy && !external_proxy_preserved {
                    self.inner
                        .storage
                        .save_config(&disabled)
                        .await
                        .map_err(|error| {
                            RuntimeActivationError::new(error, external_proxy_preserved)
                        })?;
                }
                if proxy_disable.is_some() {
                    complete_system_proxy_recovery(&self.system_proxy_backup_path())
                        .await
                        .map_err(|complete_error| {
                            RuntimeActivationError::new(
                                AppError::internal(format!(
                                    "core restart failed ({error}); completing proxy recovery failed ({complete_error})"
                                )),
                                external_proxy_preserved,
                            )
                        })?;
                }
                return Err(RuntimeActivationError::new(error, external_proxy_preserved));
            }
            if config.system_proxy && !external_proxy_preserved {
                apply_system_proxy(config, &self.system_proxy_backup_path()).await?;
            } else if proxy_disable.is_some() {
                complete_system_proxy_recovery(&self.system_proxy_backup_path())
                    .await
                    .map_err(|error| {
                        RuntimeActivationError::new(error, external_proxy_preserved)
                    })?;
            }
            self.synchronize_proxy_selections(config, true).await?;
            return Ok(external_proxy_preserved);
        }

        if config.tun {
            validate_tun_permissions().await?;
        }
        self.inner
            .core
            .start(self.core_start_config(config, path))
            .await?;
        if config.system_proxy {
            apply_system_proxy(config, &self.system_proxy_backup_path()).await?;
        }
        self.synchronize_proxy_selections(config, true).await?;
        Ok(false)
    }

    async fn config_update_failed(
        &self,
        error: AppError,
        previous: &SystemConfig,
        attempted: &SystemConfig,
        core_was_running: bool,
        mut external_proxy_preserved: bool,
    ) -> Result<SystemConfig, AppError> {
        warn!(error = %error, "config update failed, restoring previous state");
        let mut rollback_errors = Vec::new();
        let mut effective_previous =
            proxy_safe_rollback_config(previous, external_proxy_preserved, true);
        if let Err(rollback_error) = self.inner.storage.save_config(&effective_previous).await {
            rollback_errors.push(format!("database: {rollback_error}"));
        }
        let runtime_restored = match self
            .activate_runtime_config(&effective_previous, attempted, core_was_running)
            .await
        {
            Ok(rollback_external_proxy_preserved) => {
                external_proxy_preserved |= rollback_external_proxy_preserved;
                true
            }
            Err(rollback_error) => {
                external_proxy_preserved |= rollback_error.external_proxy_preserved;
                rollback_errors.push(format!("runtime: {}", rollback_error.error));
                false
            }
        };
        let core_restored = runtime_restored && self.inner.core.is_running().await;
        let safety_previous =
            proxy_safe_rollback_config(previous, external_proxy_preserved, core_restored);
        if effective_previous.system_proxy != safety_previous.system_proxy {
            effective_previous = safety_previous;
            if let Err(rollback_error) = self.inner.storage.save_config(&effective_previous).await {
                rollback_errors.push(format!("database proxy safety state: {rollback_error}"));
            }
        }
        if previous.system_proxy || attempted.system_proxy {
            if let Err(rollback_error) =
                apply_system_proxy(&effective_previous, &self.system_proxy_backup_path()).await
            {
                rollback_errors.push(format!("system proxy: {rollback_error}"));
            }
        }
        if rollback_errors.is_empty() {
            Err(error)
        } else {
            Err(AppError::internal(format!(
                "config update failed ({error}); rollback failed ({})",
                rollback_errors.join("; ")
            )))
        }
    }

    async fn restore_proxy_intent_after_failed_stop(
        &self,
        enabled: &SystemConfig,
        disabled: &SystemConfig,
    ) -> Result<(), AppError> {
        self.inner.storage.save_config(enabled).await?;
        if let Err(error) = apply_system_proxy(enabled, &self.system_proxy_backup_path()).await {
            let safety_persistence = self.inner.storage.save_config(disabled).await;
            return Err(AppError::internal(format!(
                "re-enabling system proxy failed ({error}); restoring disabled safety intent: {safety_persistence:?}"
            )));
        }
        Ok(())
    }

    async fn begin_temporary_system_proxy_disable(
        &self,
        enabled: &SystemConfig,
        disabled: &SystemConfig,
    ) -> Result<Option<SystemProxyRestoreOutcome>, AppError> {
        match begin_system_proxy_disable(disabled, &self.system_proxy_backup_path()).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                match apply_system_proxy(enabled, &self.system_proxy_backup_path()).await {
                    Ok(()) => Err(error),
                    Err(rollback_error) => {
                        let safety_persistence = self.inner.storage.save_config(disabled).await;
                        Err(AppError::internal(format!(
                            "disabling system proxy failed ({error}); re-enabling it failed ({rollback_error}); disabled safety intent: {safety_persistence:?}"
                        )))
                    }
                }
            }
        }
    }

    async fn refresh_startup_assets(&self) {
        let due_subs = self
            .inner
            .storage
            .startup_subscription_ids()
            .await
            .unwrap_or_default();
        if !due_subs.is_empty() {
            info!(
                count = due_subs.len(),
                "refreshing startup-due subscriptions"
            );
        }
        for id in due_subs {
            if let Err(err) = self.refresh_subscription(&id).await {
                warn!("startup subscription refresh failed for {id}: {err}");
            }
        }

        let ids = self.startup_rule_set_ids().await.unwrap_or_default();
        if !ids.is_empty() {
            info!(
                count = ids.len(),
                "refreshing startup-due rule sets before runtime compile"
            );
        }
        for id in ids {
            if let Err(err) = self.refresh_rule_set(&id).await {
                warn!("startup rule set refresh failed for {id}: {err}");
            }
        }
    }

    async fn startup_rule_set_ids(&self) -> Result<Vec<String>, AppError> {
        let due = self
            .inner
            .storage
            .due_rule_set_ids()
            .await?
            .into_iter()
            .collect::<HashSet<_>>();
        let mut ids = Vec::new();
        for rule_set in self.inner.storage.rule_sets_for_runtime().await? {
            let missing_local_file = rule_set
                .local_path
                .as_deref()
                .map(|path| !self.inner.paths.resolve_local_path(path).is_file())
                .unwrap_or(true);
            if missing_local_file || due.contains(&rule_set.id) {
                ids.push(rule_set.id);
            }
        }
        Ok(ids)
    }

    async fn start_background_tasks(&self) {
        let app = self.clone();
        let _ = self
            .inner
            .background_started
            .get_or_init(|| async move {
                info!("starting background task loop");
                let watchdog = app.clone();
                tokio::spawn(async move {
                    watchdog.system_proxy_watchdog().await;
                });
                tokio::spawn(async move {
                    app.refresh_startup_assets().await;
                    app.background_loop().await;
                });
            })
            .await;
    }

    async fn system_proxy_watchdog(self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let Ok(config) = self.config().await else {
                continue;
            };
            let backup_found =
                match system_proxy_backup_exists(&self.system_proxy_backup_path()).await {
                    Ok(found) => found,
                    Err(error) => {
                        warn!(%error, "failed to check system proxy recovery state; retrying");
                        continue;
                    }
                };
            if !system_proxy_recovery_required(&config, backup_found) {
                continue;
            }
            if config.system_proxy {
                let _ = self
                    .inner
                    .core
                    .snapshot(config.external_controller.clone())
                    .await;
                if self.inner.core.is_running().await {
                    continue;
                }
            }

            let _config_update = self.inner.config_update.lock().await;
            let _runtime_operation = self.inner.runtime_operation.lock().await;
            let Ok(mut current) = self.config().await else {
                continue;
            };
            let backup_found =
                match system_proxy_backup_exists(&self.system_proxy_backup_path()).await {
                    Ok(found) => found,
                    Err(error) => {
                        warn!(%error, "failed to recheck system proxy recovery state; retrying");
                        continue;
                    }
                };
            if !system_proxy_recovery_required(&current, backup_found) {
                continue;
            }
            if current.system_proxy && self.inner.core.is_running().await {
                continue;
            }
            let proxy_intent_was_enabled = current.system_proxy;
            current.system_proxy = false;
            if proxy_intent_was_enabled {
                if let Err(error) = self.inner.storage.save_config(&current).await {
                    warn!(%error, "failed to persist system proxy cleanup after core exit; retrying");
                    continue;
                }
            }
            match apply_system_proxy(&current, &self.system_proxy_backup_path()).await {
                Ok(()) => {
                    if proxy_intent_was_enabled {
                        warn!("system proxy was disabled because the managed core exited");
                    } else {
                        warn!("recovered an interrupted system proxy transaction");
                    }
                }
                Err(error) => {
                    warn!(%error, "failed to recover or disable the managed system proxy; retrying");
                }
            }
        }
    }

    async fn background_loop(self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let due_subs = self
                .inner
                .storage
                .due_subscription_ids()
                .await
                .unwrap_or_default();
            if !due_subs.is_empty() {
                info!(count = due_subs.len(), "found due subscriptions");
            }
            for id in due_subs {
                if let Err(err) = self.refresh_subscription(&id).await {
                    warn!("subscription auto refresh failed for {id}: {err}");
                }
            }
            let due_rule_sets = self
                .inner
                .storage
                .due_rule_set_ids()
                .await
                .unwrap_or_default();
            if !due_rule_sets.is_empty() {
                info!(count = due_rule_sets.len(), "found due rule sets");
            }
            for id in due_rule_sets {
                if let Err(err) = self.refresh_rule_set(&id).await {
                    warn!("rule set auto refresh failed for {id}: {err}");
                }
            }
            match self.inner.backup_service.auto_sync_due().await {
                Ok(true) => {
                    if let Err(error) = self.inner.backup_service.sync_to_webdav().await {
                        warn!(%error, "automatic WebDAV backup failed");
                    }
                }
                Ok(false) => {}
                Err(error) => warn!(%error, "failed to evaluate automatic WebDAV backup"),
            }
        }
    }

    pub async fn append_app_log(&self, level: &str, payload: &str) {
        let parsed = parse_host_from_log(payload);
        let _ = self
            .inner
            .storage
            .append_log(level, payload, parsed.as_deref())
            .await;
    }
}

fn disable_system_proxy_intent(config: &mut SystemConfig) -> bool {
    if !config.system_proxy {
        return false;
    }
    config.system_proxy = false;
    true
}

fn startup_recovery_disables_proxy_intent(
    external_changes_preserved: bool,
    recovery_failed: bool,
) -> bool {
    external_changes_preserved || recovery_failed
}

fn egress_proxy_url(core_state: &str, mixed_port: u16) -> Option<String> {
    (core_state == "running").then(|| format!("http://127.0.0.1:{mixed_port}"))
}

fn requires_early_system_proxy_persist(current: &SystemConfig, attempted: &SystemConfig) -> bool {
    current.system_proxy != attempted.system_proxy
}

fn runtime_change_requires_restart(current: &SystemConfig, attempted: &SystemConfig) -> bool {
    current.tun || attempted.tun
}

fn system_proxy_recovery_required(config: &SystemConfig, backup_found: bool) -> bool {
    config.system_proxy || backup_found
}

fn proxy_disable_preserved_external(outcome: &Option<SystemProxyRestoreOutcome>) -> bool {
    outcome
        .as_ref()
        .is_some_and(|outcome| outcome.external_changes_preserved)
}

fn proxy_safe_rollback_config(
    previous: &SystemConfig,
    external_proxy_preserved: bool,
    core_restored: bool,
) -> SystemConfig {
    let mut effective = previous.clone();
    if external_proxy_preserved || (effective.system_proxy && !core_restored) {
        effective.system_proxy = false;
    }
    effective
}

fn validate_config(config: &SystemConfig) -> Result<(), AppError> {
    if config.mixed_port == 0 {
        return Err(AppError::bad_request(
            "config_invalid",
            "mixed_port must be between 1 and 65535",
        ));
    }
    if !matches!(config.mode.as_str(), "rule" | "global" | "direct") {
        return Err(AppError::bad_request(
            "config_invalid",
            format!("unsupported mode {}", config.mode),
        ));
    }
    if !matches!(
        config.log_level.as_str(),
        "silent" | "error" | "warning" | "info" | "debug"
    ) {
        return Err(AppError::bad_request(
            "config_invalid",
            format!("unsupported log level {}", config.log_level),
        ));
    }
    if !matches!(config.dns_mode.as_str(), "fake-ip" | "redir-host") {
        return Err(AppError::bad_request(
            "config_invalid",
            format!("unsupported dns mode {}", config.dns_mode),
        ));
    }
    let delay_url = reqwest::Url::parse(&config.delay_test_url).map_err(|_| {
        AppError::bad_request(
            "config_invalid_delay_test_url",
            "delay_test_url must be a valid URL",
        )
    })?;
    if delay_url.scheme() != "https" {
        return Err(AppError::bad_request(
            "config_invalid_delay_test_url",
            "delay_test_url must use HTTPS",
        ));
    }
    if !(1_000..=60_000).contains(&config.delay_test_timeout_ms) {
        return Err(AppError::bad_request(
            "config_invalid_delay_timeout",
            "delay_test_timeout_ms must be between 1000 and 60000",
        ));
    }
    validate_dns_config(config)?;
    if config.external_controller_enabled {
        let controller = parse_controller_url(&config.external_controller).ok_or_else(|| {
            AppError::bad_request(
                "config_invalid",
                "external_controller must be a valid host and port",
            )
        })?;
        if controller.port_or_known_default().is_none() {
            return Err(AppError::bad_request(
                "config_invalid",
                "external_controller must include a valid port",
            ));
        }
        if !controller_host_is_loopback(&controller) && config.secret.trim().len() < 16 {
            return Err(AppError::bad_request(
                "config_invalid",
                "a non-loopback external_controller requires a secret of at least 16 characters",
            ));
        }
    }
    Ok(())
}

fn validate_dns_config(config: &SystemConfig) -> Result<(), AppError> {
    if config.dns_enabled && config.dns_nameservers.is_empty() {
        return Err(AppError::bad_request(
            "config_invalid_dns",
            "at least one DNS nameserver is required when DNS is enabled",
        ));
    }
    for (label, values) in [
        ("nameserver", &config.dns_nameservers),
        ("fallback", &config.dns_fallback),
    ] {
        if values.len() > 128 || values.iter().any(|value| !valid_dns_server(value)) {
            return Err(AppError::bad_request(
                "config_invalid_dns",
                format!("{label} contains an invalid DNS server"),
            ));
        }
    }
    if config.dns_fake_ip_filter.len() > 2048
        || config
            .dns_fake_ip_filter
            .iter()
            .any(|value| !valid_dns_token(value))
    {
        return Err(AppError::bad_request(
            "config_invalid_dns",
            "fake-IP filter contains an invalid entry",
        ));
    }
    validate_dns_map("nameserver policy", &config.dns_nameserver_policy, true)?;
    validate_dns_map("hosts", &config.dns_hosts, false)?;
    Ok(())
}

fn validate_dns_map(
    label: &str,
    values: &std::collections::BTreeMap<String, Vec<String>>,
    servers: bool,
) -> Result<(), AppError> {
    if values.len() > 512 {
        return Err(AppError::bad_request(
            "config_invalid_dns",
            format!("{label} contains too many entries"),
        ));
    }
    for (key, entries) in values {
        if !valid_dns_token(key)
            || entries.is_empty()
            || entries.len() > 32
            || entries.iter().any(|entry| {
                if servers {
                    !valid_dns_server(entry)
                } else {
                    !valid_dns_token(entry)
                }
            })
        {
            return Err(AppError::bad_request(
                "config_invalid_dns",
                format!("{label} contains an invalid entry for {key}"),
            ));
        }
    }
    Ok(())
}

fn valid_dns_server(value: &str) -> bool {
    let value = value.trim();
    if !valid_dns_token(value) {
        return false;
    }
    if matches!(value, "system" | "dhcp://system") {
        return true;
    }
    if let Ok(url) = reqwest::Url::parse(value) {
        return matches!(
            url.scheme(),
            "udp" | "tcp" | "tls" | "https" | "quic" | "dhcp" | "rcode"
        ) && url.host_str().is_some();
    }
    let host = value.split_once('#').map(|(host, _)| host).unwrap_or(value);
    host.parse::<std::net::IpAddr>().is_ok()
        || host.parse::<std::net::SocketAddr>().is_ok()
        || (!host.contains('/') && host.contains('.'))
}

fn valid_dns_token(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 2048
        && !value.chars().any(char::is_control)
        && !value.contains(',')
}

fn parse_controller_url(value: &str) -> Option<reqwest::Url> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") {
        reqwest::Url::parse(value).ok()
    } else {
        reqwest::Url::parse(&format!("http://{value}")).ok()
    }
}

fn controller_host_is_loopback(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn ensure_runtime_ports_available(config: &SystemConfig) -> Result<(), AppError> {
    if !port_available(config.mixed_port).await {
        return Err(AppError::conflict(
            "port_in_use",
            format!(
                "mixed proxy port {} is already in use. Change the port in Settings or close the other program.",
                config.mixed_port
            ),
        ));
    }
    if !controller_port_available(config).await {
        return Err(AppError::conflict(
            "controller_port_in_use",
            format!(
                "external controller {} is already in use. Change the controller address in Settings or close the other program.",
                config.external_controller
            ),
        ));
    }
    Ok(())
}

async fn controller_port_available(config: &SystemConfig) -> bool {
    if !config.external_controller_enabled {
        return true;
    }
    if let Some(port) = parse_controller_port(&config.external_controller) {
        port_available(port).await
    } else {
        true
    }
}

fn parse_controller_port(value: &str) -> Option<u16> {
    parse_controller_url(value)?.port_or_known_default()
}

async fn port_available(port: u16) -> bool {
    tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .is_ok()
}

fn validate_subscription_input(input: &SubscriptionInput) -> Result<(), AppError> {
    if !validate_url(input.url.trim()) {
        return Err(AppError::bad_request(
            "subscription_invalid_url",
            "subscription url must start with http:// or https://",
        ));
    }
    for rule in &input.rules {
        validate_filter_rule_input(rule)?;
    }
    Ok(())
}

fn validate_filter_rule_input(rule: &FilterRuleInput) -> Result<(), AppError> {
    if !matches!(rule.action.trim(), "keep" | "include" | "引入" | "discard") {
        return Err(AppError::bad_request(
            "subscription_rule_invalid",
            format!("unsupported subscription filter action {}", rule.action),
        ));
    }
    if !matches!(
        rule.match_type.trim(),
        "contains" | "not_contains" | "notContains" | "regex" | "in" | "equals"
    ) {
        return Err(AppError::bad_request(
            "subscription_rule_invalid",
            format!("unsupported subscription filter type {}", rule.match_type),
        ));
    }
    let match_type = rule.match_type.trim();
    let has_match_value = if matches!(match_type, "in" | "equals") {
        !rule.is_pattern_empty()
    } else {
        !rule.pattern.trim().is_empty()
    };
    if !has_match_value {
        return Err(AppError::bad_request(
            "subscription_rule_invalid",
            "subscription filter pattern cannot be empty",
        ));
    }
    if rule.match_type.trim() == "regex" && regex::Regex::new(rule.pattern.trim()).is_err() {
        return Err(AppError::bad_request(
            "subscription_rule_invalid_regex",
            format!("invalid subscription filter regex {}", rule.pattern),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use axum::Router;
    use std::path::Path;
    use std::time::Duration;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rweb-clash-app-{label}-{}",
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).expect("create app test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn test_app(root: &Path) -> App {
        let global_lock_path = root.join("test-global-app.lock");
        try_test_app(root, &global_lock_path)
            .await
            .expect("create test app")
    }

    async fn try_test_app(root: &Path, global_lock_path: &Path) -> Result<App, AppError> {
        let paths = AppPaths::from_root(root);
        paths.ensure_dirs()?;
        let global_app_lock = GlobalAppLock::acquire_at(global_lock_path)?;
        let data_root_lock = DataRootLock::acquire(&paths)?;
        let storage = Storage::connect(&paths).await?;
        let core = CoreManager::new(paths.clone(), storage.clone());
        let backup_service = BackupService::new(storage.clone(), paths.clone());
        Ok(App {
            inner: Arc::new(AppInner {
                paths: paths.clone(),
                embedded_assets: None,
                subscription_syncer: SubscriptionSyncer::new(storage.clone(), paths.clone()),
                proxy_service: ProxyService::new(storage.clone()),
                rule_service: RuleService::new(storage.clone(), paths),
                egress_probe: EgressProbe::new(),
                backup_service,
                storage,
                core,
                config_update: Mutex::new(()),
                runtime_operation: Mutex::new(()),
                rule_set_operation: Mutex::new(()),
                background_started: OnceCell::new(),
                _data_root_lock: data_root_lock,
                _global_app_lock: global_app_lock,
            }),
        })
    }

    #[tokio::test]
    async fn initialize_rejects_a_second_data_root_instance_before_storage_or_cleanup() {
        let temp = TestDir::new("second-data-root-instance");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let stale_candidate = paths
            .profiles_dir
            .join(".subscription-candidate-interrupted.yaml");
        std::fs::write(&stale_candidate, b"sensitive candidate").expect("write stale candidate");
        let _first_instance = DataRootLock::acquire(&paths).expect("acquire first instance lock");
        let global_lock_path = temp.path().join("test-global-app.lock");
        let global_app_lock =
            GlobalAppLock::acquire_at(&global_lock_path).expect("acquire test global lock");

        let error = App::initialize_with_global_lock(
            AppOptions {
                root_dir: Some(temp.path().to_path_buf()),
                ..AppOptions::default()
            },
            paths.clone(),
            global_app_lock,
        )
        .await
        .expect_err("reject a second instance");

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code, "data_root_in_use");
        assert!(stale_candidate.is_file());
        assert!(!paths.database_file.exists());
    }

    #[tokio::test]
    async fn initialize_retries_cleanup_of_an_orphan_rule_set_snapshot() {
        let temp = TestDir::new("orphan-rule-set-startup-cleanup");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let orphan = paths.rule_sets_dir.join("rs_deleted.list");
        std::fs::write(&orphan, b"example.com\n").expect("write orphan snapshot");
        let global_lock_path = temp.path().join("test-global-app.lock");
        let global_app_lock =
            GlobalAppLock::acquire_at(&global_lock_path).expect("acquire test global lock");

        let _app = App::initialize_with_global_lock(
            AppOptions {
                root_dir: Some(temp.path().to_path_buf()),
                ..AppOptions::default()
            },
            paths,
            global_app_lock,
        )
        .await
        .expect("initialize app");

        assert!(!orphan.exists());
    }

    #[tokio::test]
    async fn initialize_keeps_core_stopped_when_auto_start_is_disabled() {
        let temp = TestDir::new("initialize-core-stopped");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let global_lock_path = temp.path().join("test-global-app.lock");
        let global_app_lock =
            GlobalAppLock::acquire_at(&global_lock_path).expect("acquire test global lock");

        let app = App::initialize_with_global_lock(
            AppOptions {
                root_dir: Some(temp.path().to_path_buf()),
                ..AppOptions::default()
            },
            paths,
            global_app_lock,
        )
        .await
        .expect("initialize app");

        assert!(!app.config().await.expect("load config").auto_start);
        assert_eq!(
            app.core_status().await.expect("load core status").state,
            "not_running"
        );
    }

    #[tokio::test]
    async fn global_app_lock_rejects_an_app_using_a_different_data_root() {
        let temp = TestDir::new("global-lock-different-roots");
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        let global_lock_path = temp.path().join("shared-global-app.lock");
        let first = try_test_app(&first_root, &global_lock_path)
            .await
            .expect("create first app");

        let error = try_test_app(&second_root, &global_lock_path)
            .await
            .expect_err("reject second app with another data root");

        assert_eq!(error.status, axum::http::StatusCode::CONFLICT);
        assert_eq!(error.code, "app_instance_in_use");
        drop(first);
        let _second = try_test_app(&second_root, &global_lock_path)
            .await
            .expect("acquire global app lock after first app drops");
    }

    #[test]
    fn non_loopback_controller_requires_a_strong_secret() {
        let mut config = SystemConfig {
            external_controller: "0.0.0.0:9090".into(),
            secret: String::new(),
            ..SystemConfig::default()
        };
        assert!(validate_config(&config).is_err());

        config.secret = "0123456789abcdef".into();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn loopback_controller_allows_an_empty_secret() {
        let config = SystemConfig {
            external_controller: "[::1]:9090".into(),
            secret: String::new(),
            ..SystemConfig::default()
        };
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn changing_tun_mode_requires_a_core_restart() {
        let current = SystemConfig::default();
        let mut attempted = current.clone();
        assert!(!runtime_change_requires_restart(&current, &attempted));

        attempted.tun = true;
        assert!(runtime_change_requires_restart(&current, &attempted));
        assert!(runtime_change_requires_restart(&attempted, &current));
        assert!(runtime_change_requires_restart(&attempted, &attempted));
    }

    #[test]
    fn clean_crash_recovery_keeps_persisted_proxy_intent() {
        let config = SystemConfig {
            system_proxy: true,
            auto_start: false,
            ..SystemConfig::default()
        };

        assert!(!startup_recovery_disables_proxy_intent(false, false));
        assert!(config.system_proxy);
        assert!(!config.auto_start);
    }

    #[test]
    fn egress_uses_mihomo_only_while_the_core_is_running() {
        assert_eq!(
            egress_proxy_url("running", 7890).as_deref(),
            Some("http://127.0.0.1:7890")
        );
        for state in ["not_running", "starting", "stopping", "error"] {
            assert_eq!(egress_proxy_url(state, 7890), None);
        }
    }

    #[test]
    fn external_takeover_or_recovery_failure_disables_proxy_intent() {
        let mut config = SystemConfig {
            system_proxy: true,
            ..SystemConfig::default()
        };
        assert!(startup_recovery_disables_proxy_intent(true, false));
        assert!(disable_system_proxy_intent(&mut config));
        assert!(!config.system_proxy);
        assert!(startup_recovery_disables_proxy_intent(false, true));
    }

    #[test]
    fn proxy_intent_transition_is_persisted_before_runtime_can_touch_the_os() {
        let disabled = SystemConfig::default();
        let mut enabled = disabled.clone();
        enabled.system_proxy = true;

        assert!(requires_early_system_proxy_persist(&disabled, &enabled));
        assert!(requires_early_system_proxy_persist(&enabled, &disabled));
        assert!(!requires_early_system_proxy_persist(&enabled, &enabled));
    }

    #[test]
    fn subscription_filter_enums_reject_silent_fallbacks() {
        let valid = FilterRuleInput {
            action: "keep".into(),
            match_type: "contains".into(),
            pattern: "HK".into(),
            ..FilterRuleInput::default()
        };
        assert!(validate_filter_rule_input(&valid).is_ok());

        let mut invalid_action = valid.clone();
        invalid_action.action = "kepe".into();
        assert_eq!(
            validate_filter_rule_input(&invalid_action)
                .expect_err("reject an action typo")
                .code,
            "subscription_rule_invalid"
        );

        let mut invalid_type = valid;
        invalid_type.match_type = "contain".into();
        assert_eq!(
            validate_filter_rule_input(&invalid_type)
                .expect_err("reject a match type typo")
                .code,
            "subscription_rule_invalid"
        );

        for match_type in ["contains", "not_contains", "notContains", "regex"] {
            let invalid_values_only = FilterRuleInput {
                action: "keep".into(),
                match_type: match_type.into(),
                pattern: String::new(),
                values: vec!["HK".into()],
                ..FilterRuleInput::default()
            };
            assert_eq!(
                validate_filter_rule_input(&invalid_values_only)
                    .expect_err("unrelated values must not hide an empty pattern")
                    .code,
                "subscription_rule_invalid"
            );
        }

        for match_type in ["in", "equals"] {
            let valid_values_only = FilterRuleInput {
                action: "keep".into(),
                match_type: match_type.into(),
                pattern: String::new(),
                values: vec!["HK".into()],
                ..FilterRuleInput::default()
            };
            assert!(validate_filter_rule_input(&valid_values_only).is_ok());
        }
    }

    #[test]
    fn watchdog_and_shutdown_retry_a_backup_even_after_proxy_intent_is_disabled() {
        let disabled = SystemConfig::default();
        let enabled = SystemConfig {
            system_proxy: true,
            ..SystemConfig::default()
        };

        assert!(system_proxy_recovery_required(&disabled, true));
        assert!(!system_proxy_recovery_required(&disabled, false));
        assert!(system_proxy_recovery_required(&enabled, false));
    }

    #[test]
    fn external_disable_outcome_blocks_enabled_intent_rollback() {
        let external = Some(SystemProxyRestoreOutcome {
            external_changes_preserved: true,
        });
        let clean = Some(SystemProxyRestoreOutcome {
            external_changes_preserved: false,
        });
        let previous = SystemConfig {
            system_proxy: true,
            ..SystemConfig::default()
        };

        assert!(proxy_disable_preserved_external(&external));
        assert!(!proxy_disable_preserved_external(&clean));
        assert!(!proxy_disable_preserved_external(&None));
        assert!(!proxy_safe_rollback_config(&previous, true, true).system_proxy);
        assert!(proxy_safe_rollback_config(&previous, false, true).system_proxy);
        assert!(!proxy_safe_rollback_config(&previous, false, false).system_proxy);
    }

    #[tokio::test]
    async fn flush_dns_preserves_controller_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake controller");
        let address = listener.local_addr().expect("fake controller address");
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/cache/fakeip/flush",
                    post(|| async { axum::http::StatusCode::BAD_GATEWAY }),
                ),
            )
            .await
        });

        let temp = TestDir::new("flush-dns-controller-error");
        let app = test_app(temp.path()).await;
        let mut config = app.config().await.expect("load config");
        config.external_controller = address.to_string();
        config.secret.clear();
        app.inner
            .storage
            .save_config(&config)
            .await
            .expect("save fake controller address");

        let error = app
            .flush_controller_dns()
            .await
            .expect_err("controller error must be returned");

        assert_eq!(error.status, axum::http::StatusCode::BAD_GATEWAY);
        assert_eq!(error.code, "controller_unexpected_status");
        server.abort();
    }

    #[tokio::test]
    async fn subscription_update_waits_for_refresh_before_snapshot_and_write() {
        let temp = TestDir::new("subscription-update-lock");
        let app = test_app(temp.path()).await;
        let subscription_id = "sub_serialized";
        app.inner
            .storage
            .create_subscription(
                subscription_id,
                "Original",
                "http://127.0.0.1:1/original",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");

        let refresh_guard = app
            .inner
            .subscription_syncer
            .lock_refresh(subscription_id)
            .await;
        let update_app = app.clone();
        let mut update = tokio::spawn(async move {
            update_app
                .update_subscription(
                    subscription_id,
                    SubscriptionInput {
                        name: "Candidate".into(),
                        url: "http://127.0.0.1:1/candidate".into(),
                        interval_seconds: Some(7200),
                        interval: None,
                        inherit_global: Some(false),
                        rules: Vec::new(),
                        download_route: crate::types::DownloadRoute::Auto,
                    },
                )
                .await
        });

        assert!(tokio::time::timeout(Duration::from_millis(50), &mut update)
            .await
            .is_err());
        let while_locked = app.list_subscriptions().await.expect("list subscriptions");
        assert_eq!(while_locked[0].name, "Original");

        app.inner
            .storage
            .update_subscription(
                subscription_id,
                "Refreshed",
                "http://127.0.0.1:1/refreshed",
                5400,
                true,
                &[],
            )
            .await
            .expect("simulate the in-flight refresh commit");
        drop(refresh_guard);

        let update_result = tokio::time::timeout(Duration::from_secs(10), update)
            .await
            .expect("subscription update completed")
            .expect("subscription update task did not panic");
        assert!(update_result.is_err());
        let after_rollback = app.list_subscriptions().await.expect("list subscriptions");
        assert_eq!(after_rollback[0].name, "Refreshed");
        assert_eq!(after_rollback[0].url, "http://127.0.0.1:1/refreshed");
    }

    #[tokio::test]
    async fn failed_initial_subscription_refresh_rolls_back_the_new_record() {
        let temp = TestDir::new("subscription-create-rollback");
        let app = test_app(temp.path()).await;
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            app.create_subscription(SubscriptionInput {
                name: "Unreachable".into(),
                url: "http://127.0.0.1:1/profile.yaml".into(),
                interval_seconds: Some(3_600),
                interval: None,
                inherit_global: Some(true),
                rules: Vec::new(),
                download_route: crate::types::DownloadRoute::Auto,
            }),
        )
        .await
        .expect("initial refresh should fail promptly")
        .expect_err("unreachable subscription must fail");
        assert!(matches!(
            result.code.as_str(),
            "network_unreachable" | "remote_private_address_blocked" | "subscription_fetch_failed"
        ));
        assert!(app
            .list_subscriptions()
            .await
            .expect("list subscriptions after rollback")
            .is_empty());
    }
}
