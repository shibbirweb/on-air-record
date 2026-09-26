/**
 * The question every install is asked once: protect it with a login, or keep it open.
 *
 * Shown to whoever opens the page first, over the running app, and it cannot be dismissed without an
 * answer. Choosing to stay open is remembered by the server, so the question never returns; accounts can
 * still be switched on later from the settings page.
 */

import { LockKeyhole, LockKeyholeOpen, Loader2 } from 'lucide-react';
import { useState } from 'react';

import { ApiError } from '@/api/client';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { CredentialsForm } from '@/features/auth/CredentialsForm';
import { capitalise } from '@/lib/credentials';
import { useAuthStore } from '@/store/useAuthStore';

export function FirstRunDialog() {
  const chooseOpen = useAuthStore((state) => state.chooseOpen);
  const setUp = useAuthStore((state) => state.setUp);
  const [step, setStep] = useState<'choose' | 'set-up'>('choose');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const keepOpen = async () => {
    setBusy(true);
    setError(null);
    try {
      await chooseOpen();
    } catch (cause) {
      // Somebody else answered first in another browser. Their answer stands; fetch it.
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
      void useAuthStore.getState().refresh();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open>
      <DialogContent dismissible={false}>
        {step === 'choose' ? (
          <>
            <DialogHeader>
              <DialogTitle>Protect this recorder with a login?</DialogTitle>
              <DialogDescription>
                Right now anyone who can reach this page can listen live, go back through the recordings,
                download them, and change the settings.
              </DialogDescription>
            </DialogHeader>

            <div className="grid gap-3">
              <button
                type="button"
                className="hover:bg-accent focus-visible:ring-ring/50 flex items-start gap-3 rounded-lg border p-4 text-left outline-none focus-visible:ring-[3px]"
                onClick={() => setStep('set-up')}
              >
                <LockKeyhole className="text-primary mt-0.5 size-5 shrink-0" />
                <span className="space-y-1">
                  <span className="block text-sm font-medium">Set up accounts</span>
                  <span className="text-muted-foreground block text-xs">
                    Everyone signs in. You become the admin, and can add people who may only listen.
                  </span>
                </span>
              </button>

              <button
                type="button"
                disabled={busy}
                className="hover:bg-accent focus-visible:ring-ring/50 flex items-start gap-3 rounded-lg border p-4 text-left outline-none focus-visible:ring-[3px] disabled:opacity-50"
                onClick={() => void keepOpen()}
              >
                {busy ? (
                  <Loader2 className="mt-0.5 size-5 shrink-0 animate-spin" />
                ) : (
                  <LockKeyholeOpen className="text-muted-foreground mt-0.5 size-5 shrink-0" />
                )}
                <span className="space-y-1">
                  <span className="block text-sm font-medium">Keep it open</span>
                  <span className="text-muted-foreground block text-xs">
                    No login, as before. Fine on a network where you trust everyone. You can switch
                    accounts on later in Settings.
                  </span>
                </span>
              </button>
            </div>

            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}
          </>
        ) : (
          <>
            <DialogHeader>
              <DialogTitle>Create the admin account</DialogTitle>
              <DialogDescription>
                This is the account that controls the recorder and adds everyone else. There is no email
                sent anywhere; the address is only what you sign in with.
              </DialogDescription>
            </DialogHeader>

            <CredentialsForm submitLabel="Create and sign in" newPassword onSubmit={setUp} />

            <Button variant="ghost" size="sm" onClick={() => setStep('choose')}>
              Back
            </Button>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
