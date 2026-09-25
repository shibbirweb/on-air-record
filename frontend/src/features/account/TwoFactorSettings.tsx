/**
 * Your own two factor sign in, on the account settings page: whether it is on, and the way into setting it
 * up or changing it. The steps themselves live in `TwoFactorDialog`.
 */

import { ShieldCheck, ShieldOff } from 'lucide-react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { TwoFactorDialog } from '@/features/auth/TwoFactorDialog';
import { useTwoFactorStore } from '@/store/useTwoFactorStore';

export function TwoFactorSettings() {
  const status = useTwoFactorStore((state) => state.status);
  const refresh = useTwoFactorStore((state) => state.refresh);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="flex flex-wrap items-center justify-between gap-4">
      <div className="flex items-start gap-3">
        {status?.enabled ? (
          <ShieldCheck className="text-primary mt-0.5 size-5 shrink-0" />
        ) : (
          <ShieldOff className="text-muted-foreground mt-0.5 size-5 shrink-0" />
        )}
        <div className="space-y-1">
          <p className="text-sm font-medium">
            {status === null ? 'Checking...' : status.enabled ? 'On' : 'Off'}
          </p>
          <p className="text-muted-foreground text-xs">
            {status?.enabled
              ? `Signing in asks for a code from your authenticator app. ${status.recoveryCodesLeft} of 10 recovery codes left.`
              : 'Signing in needs only your password. Add a code from an authenticator app on your phone.'}
          </p>
        </div>
      </div>

      <Button variant={status?.enabled ? 'outline' : 'default'} onClick={() => setOpen(true)}>
        {status?.enabled ? 'Manage' : 'Set up'}
      </Button>

      <TwoFactorDialog
        open={open}
        onOpenChange={(isOpen) => {
          setOpen(isOpen);
          if (!isOpen) {
            void refresh();
          }
        }}
      />
    </div>
  );
}
