import { useEffect, useRef, useState } from 'react';

export const DEFAULT_PAGE_IDLE_TIMEOUT_MS = 120_000;

const ACTIVITY_EVENTS = ['pointerdown', 'pointermove', 'keydown', 'wheel', 'touchstart'] as const;
const TIMER_RESET_THROTTLE_MS = 1_000;

export function usePageActivity(timeoutMs = DEFAULT_PAGE_IDLE_TIMEOUT_MS) {
  const [isActive, setIsActive] = useState(true);
  const isActiveRef = useRef(true);
  const timerRef = useRef<number | null>(null);
  const lastTimerResetRef = useRef(0);

  useEffect(() => {
    const clearIdleTimer = () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };

    const markInactive = () => {
      clearIdleTimer();
      lastTimerResetRef.current = 0;
      if (isActiveRef.current) {
        isActiveRef.current = false;
        setIsActive(false);
      }
    };

    const armIdleTimer = () => {
      clearIdleTimer();
      timerRef.current = window.setTimeout(markInactive, timeoutMs);
    };

    const markActive = () => {
      if (document.hidden) return;

      const now = Date.now();
      if (!isActiveRef.current) {
        isActiveRef.current = true;
        setIsActive(true);
      }

      if (now - lastTimerResetRef.current >= TIMER_RESET_THROTTLE_MS) {
        lastTimerResetRef.current = now;
        armIdleTimer();
      }
    };

    const handleVisibilityChange = () => {
      if (document.hidden) markInactive();
    };

    markActive();
    document.addEventListener('visibilitychange', handleVisibilityChange);
    ACTIVITY_EVENTS.forEach(eventName => {
      window.addEventListener(eventName, markActive, { passive: true });
    });

    return () => {
      clearIdleTimer();
      document.removeEventListener('visibilitychange', handleVisibilityChange);
      ACTIVITY_EVENTS.forEach(eventName => {
        window.removeEventListener(eventName, markActive);
      });
    };
  }, [timeoutMs]);

  return isActive;
}
