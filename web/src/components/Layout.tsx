import { useState, useEffect, useRef } from 'react';
import { 
  LayoutDashboard, 
  Globe, 
  Settings, 
  Rss, 
  Activity, 
  ShieldCheck,
  Zap,
  ArrowUpRight,
  ArrowDownLeft,
  Menu,
  X
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { NavLink, useLocation } from 'react-router-dom';
import { api, type CoreStatus, type Traffic } from '@/lib/api';
import { usePageActivity } from '@/lib/usePageActivity';

const LAYOUT_STATUS_POLL_MS = 10000;

interface LayoutProps {
  children: React.ReactNode;
}

const SidebarItem = ({ icon: Icon, label, to }: { 
  icon: LucideIcon,
  label: string, 
  to: string
}) => (
  <NavLink
    to={to}
    className={({ isActive }) => cn(
      "flex items-center gap-3 px-3 py-2 rounded-lg transition-all duration-200 group w-full",
      isActive 
        ? "bg-primary text-primary-foreground shadow-lg shadow-primary/20" 
        : "text-muted-foreground hover:bg-muted hover:text-foreground"
    )}
  >
    <Icon className={cn("size-5 transition-transform duration-200")} />
    <span className="font-medium whitespace-nowrap overflow-hidden">{label}</span>
  </NavLink>
);

export const Layout = ({ children }: LayoutProps) => {
  const [isSidebarOpen, setIsSidebarOpen] = useState(true);
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [traffic, setTraffic] = useState<Traffic>({ up: 0, down: 0 });
  const [coreStatus, setCoreStatus] = useState<CoreStatus | null>(null);
  const location = useLocation();
  const statusInFlight = useRef(false);
  const isPageActive = usePageActivity();

  // Handle window resize to reset states if needed
  useEffect(() => {
    const handleResize = () => {
      if (window.innerWidth >= 1024) {
        setIsMobileMenuOpen(false);
      }
    };
    window.addEventListener('resize', handleResize);
    return () => window.removeEventListener('resize', handleResize);
  }, []);

  useEffect(() => {
    if (location.pathname === '/' || !isPageActive) return;

    let disposed = false;
    const fetchStatus = async () => {
      if (!isPageActive || document.hidden || statusInFlight.current) return;
      statusInFlight.current = true;
      try {
        const [nextTraffic, nextCore] = await Promise.all([api.traffic(), api.coreStatus()]);
        if (!disposed) {
          setTraffic(nextTraffic);
          setCoreStatus(nextCore);
        }
      } catch {
        if (!disposed) {
          setTraffic({ up: 0, down: 0 });
        }
      } finally {
        statusInFlight.current = false;
      }
    };
    const handleVisibilityChange = () => {
      if (!document.hidden) void fetchStatus();
    };

    fetchStatus();
    document.addEventListener('visibilitychange', handleVisibilityChange);
    const interval = window.setInterval(fetchStatus, LAYOUT_STATUS_POLL_MS);
    return () => {
      disposed = true;
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      window.clearInterval(interval);
    };
  }, [location.pathname, isPageActive]);

  const formatSpeed = (bytes: number) => {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB/s`;
    return `${(bytes / 1024).toFixed(0)} KB/s`;
  };

  const menuItems = [
    { id: 'dashboard', label: '总览', icon: LayoutDashboard, to: '/' },
    { id: 'subscriptions', label: '订阅管理', icon: Rss, to: '/subscriptions' },
    { id: 'proxies', label: '分组管理', icon: Globe, to: '/proxies' },
    { id: 'rules', label: '路由管理', icon: ShieldCheck, to: '/rules' },
    { id: 'logs', label: '运行日志', icon: Activity, to: '/logs' },
    { id: 'settings', label: '系统设置', icon: Settings, to: '/settings' },
  ];

  const activeLabel = menuItems.find(i => 
    i.to === '/' ? location.pathname === '/' : location.pathname.startsWith(i.to)
  )?.label || 'R-Clash';

  const toggleSidebar = () => {
    if (window.innerWidth < 1024) {
      setIsMobileMenuOpen(!isMobileMenuOpen);
    } else {
      setIsSidebarOpen(!isSidebarOpen);
    }
  };

  return (
    <div className="flex h-screen bg-background overflow-hidden relative">
      {/* Mobile Backdrop */}
      {isMobileMenuOpen && (
        <div 
          className="fixed inset-0 bg-background/75 z-40 lg:hidden"
          onClick={() => setIsMobileMenuOpen(false)}
        />
      )}

      {/* Sidebar */}
      <aside className={cn(
        "bg-card shadow-sm transition-all duration-300 flex flex-col shrink-0 z-50",
        "fixed inset-y-0 left-0 lg:relative lg:translate-x-0 border-r",
        isMobileMenuOpen ? "translate-x-0 w-64 shadow-2xl" : "-translate-x-full lg:translate-x-0",
        !isMobileMenuOpen && (isSidebarOpen ? "w-64" : "w-20")
      )}>
        <div className="p-6 flex items-center justify-between lg:justify-start gap-3">
          <div className="flex items-center gap-3">
            <div className="size-8 bg-primary rounded-lg flex items-center justify-center text-primary-foreground shrink-0">
              <Zap className="size-5 fill-current" />
            </div>
            {(isSidebarOpen || isMobileMenuOpen) && <span className="font-bold text-xl tracking-tight">R-Clash</span>}
          </div>
          <Button 
            variant="ghost" 
            size="icon" 
            className="lg:hidden" 
            onClick={() => setIsMobileMenuOpen(false)}
          >
            <X className="size-5" />
          </Button>
        </div>

        <nav className="flex-1 px-4 space-y-2 mt-4 overflow-y-auto custom-scrollbar">
          {menuItems.map((item) => (
            <div key={item.id} onClick={() => setIsMobileMenuOpen(false)}>
              <SidebarItem
                icon={item.icon}
                label={(isSidebarOpen || isMobileMenuOpen) ? item.label : ''}
                to={item.to}
              />
            </div>
          ))}
        </nav>

        <div className="p-4 border-t">
          <div className={cn(
            "rounded-xl bg-muted p-3 space-y-3",
            !(isSidebarOpen || isMobileMenuOpen) && "flex flex-col items-center"
          )}>
            <div className="flex items-center justify-between w-full">
              {(isSidebarOpen || isMobileMenuOpen) && <span className="text-xs font-medium text-muted-foreground uppercase tracking-wider text-left">系统状态</span>}
              <div className={cn("size-2 rounded-full", coreStatus?.state === 'running' ? "bg-green-500 animate-pulse" : "bg-muted-foreground")} />
            </div>
            {(isSidebarOpen || isMobileMenuOpen) && (
              <div className="space-y-2">
                <div className="flex justify-between text-sm">
                  <span className="text-muted-foreground flex items-center gap-1"><ArrowDownLeft className="size-3" /> 下载</span>
                  <span className="font-mono">{formatSpeed(traffic.down)}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-muted-foreground flex items-center gap-1"><ArrowUpRight className="size-3" /> 上传</span>
                  <span className="font-mono">{formatSpeed(traffic.up)}</span>
                </div>
              </div>
            )}
          </div>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 flex flex-col min-w-0 overflow-hidden">
        {/* Top Header */}
        <header className="h-16 border-b bg-card shadow-sm flex items-center justify-between px-4 md:px-8 shrink-0">
          <div className="flex items-center gap-4">
            <Button variant="ghost" size="icon" onClick={toggleSidebar}>
              {window.innerWidth >= 1024 && isSidebarOpen ? <X className="size-5" /> : <Menu className="size-5" />}
            </Button>
            <h2 className="text-lg font-semibold truncate">{activeLabel}</h2>
          </div>
          
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-2 bg-muted px-3 py-1.5 rounded-full shrink-0">
              <span className="text-xs font-medium px-2 py-0.5 bg-primary/20 text-primary rounded-full hidden sm:inline">{coreStatus?.state ?? 'unknown'}</span>
              <span className="text-xs text-muted-foreground">{coreStatus?.version ?? 'Mihomo'}</span>
            </div>
          </div>
        </header>

        {/* Scrollable Area */}
        <div className="flex-1 overflow-y-auto p-4 md:p-8 custom-scrollbar">
          {children}
        </div>
      </main>
    </div>
  );
};
