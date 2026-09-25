/**
 * Decides what the page shows before anything else loads: the app, the login page, or the app with the
 * first run question over it.
 *
 * It sits above the app shell, so with accounts on the shell, and with it the audio engine and the
 * stream, only exists once somebody is signed in.
 */

import { Loader2 } from 'lucide-react';
import { useEffect, type ReactNode } from 'react';

import { setUnauthorizedHandler } from '@/api/client';
import { Button } from '@/components/ui/button';
import { FirstRunDialog } from '@/features/auth/FirstRunDialog';
import { LoginPage } from '@/features/auth/LoginPage';
import { useAuthStore } from '@/store/useAuthStore';

export function AuthGate({ children }: { children: ReactNode }) {
  const loaded = useAuthStore((state) => state.loaded);
  const mode = useAuthStore((state) => state.mode);
  const user = useAuthStore((state) => state.user);
  const error = useAuthStore((state) => state.error);
  const refresh = useAuthStore((state) => state.refresh);

  useEffect(() => {
    void refresh();
    // Any request that finds the session gone sends the page back through this gate.
    setUnauthorizedHandler(() => void refresh());
    return () => setUnauthorizedHandler(null);
  }, [refresh]);

  if (!loaded || mode === null) {
    return (
      <main className="bg-background text-muted-foreground grid min-h-screen place-items-center px-4">
        {error ? (
          <div className="space-y-3 text-center">
            <p className="text-sm">The recorder cannot be reached: {error}</p>
            <Button size="sm" variant="outline" onClick={() => void refresh()}>
              Try again
            </Button>
          </div>
        ) : (
          <Loader2 className="size-5 animate-spin" aria-label="Loading" />
        )}
      </main>
    );
  }

  if (mode === 'accounts' && !user) {
    return <LoginPage />;
  }

  return (
    <>
      {children}
      {mode === 'undecided' && <FirstRunDialog />}
    </>
  );
}
