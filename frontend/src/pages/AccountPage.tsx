/**
 * Account settings: things that belong to the person signed in rather than to the recorder, so every
 * account can reach it, listeners included. Opened from the account menu in the header.
 *
 * Without accounts nobody is signed in and there is no account to set, so the page is not offered and a
 * typed URL goes back to the control room.
 */

import { Navigate } from 'react-router';

import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { ChangePasswordForm } from '@/features/account/ChangePasswordForm';
import { TwoFactorSettings } from '@/features/account/TwoFactorSettings';
import { useAuthStore } from '@/store/useAuthStore';

export function AccountPage() {
  const user = useAuthStore((state) => state.user);

  if (!user) {
    return <Navigate to="/" replace />;
  }

  return (
    <main className="mx-auto max-w-3xl space-y-4 px-4 py-6">
      <div className="space-y-1">
        <h2 className="text-xl font-semibold">Account settings</h2>
        <p className="text-muted-foreground flex flex-wrap items-center gap-2 text-sm">
          {user.email}
          <Badge variant="secondary" className="font-normal">
            {user.role === 'admin' ? 'Admin' : 'Listener'}
          </Badge>
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Password</CardTitle>
          <CardDescription>
            Other browsers signed in to this account are signed out when it changes. This one stays signed
            in.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ChangePasswordForm />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Two factor sign in</CardTitle>
          <CardDescription>
            A code from an authenticator app on your phone, asked for after your password. Someone who
            learns your password still cannot sign in without your phone.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <TwoFactorSettings />
        </CardContent>
      </Card>
    </main>
  );
}
