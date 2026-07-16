import { useState, useCallback, useMemo, type ReactNode } from 'react';
import { CheckCircle2, AlertCircle, X } from 'lucide-react';
import { cn } from '@/lib/utils';
import { ToastContext, type ToastType } from './toast-context';

interface Toast {
  id: number;
  message: string;
  type: ToastType;
}

export const ToastProvider = ({ children }: { children: ReactNode }) => {
  const [toasts, setToasts] = useState<Toast[]>([]);

  const toast = useCallback((message: string, type: ToastType = 'success') => {
    const id = Date.now();
    setToasts((prev) => [...prev, { id, message, type }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 3000);
  }, []);
  const contextValue = useMemo(() => ({ toast }), [toast]);

  return (
    <ToastContext.Provider value={contextValue}>
      {children}
      <div className="fixed bottom-8 right-8 z-[100] flex flex-col gap-3">
        {toasts.map((t) => (
          <div 
            key={t.id} 
            className={cn(
              "flex items-center gap-3 px-6 py-4 rounded-2xl shadow-2xl border animate-in slide-in-from-right duration-300 min-w-[300px] backdrop-blur-md",
              t.type === 'success' ? "bg-green-500/10 border-green-500/20 text-green-500" :
              t.type === 'error' ? "bg-red-500/10 border-red-500/20 text-red-500" :
              "bg-primary/10 border-primary/20 text-primary"
            )}
          >
            {t.type === 'success' && <CheckCircle2 className="size-5" />}
            {t.type === 'error' && <AlertCircle className="size-5" />}
            <span className="font-bold text-sm flex-1">{t.message}</span>
            <button onClick={() => setToasts(prev => prev.filter(toast => toast.id !== t.id))}>
              <X className="size-4 opacity-50 hover:opacity-100" />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
};
