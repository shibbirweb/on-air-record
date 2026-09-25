/**
 * Shown instead of the app when accounts are on and nobody is signed in.
 *
 * Rendered outside the app shell on purpose, so no audio engine is built and no stream is opened until
 * somebody has signed in.
 */

import { Radio } from 'lucide-react';

import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { CredentialsForm } from '@/features/auth/CredentialsForm';
import { useAuthStore } from '@/store/useAuthStore';

export function LoginPage() {
  const logIn = useAuthStore((state) => state.logIn);

  return (
    <main className="bg-background grid min-h-screen place-items-center px-4 py-10">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex items-center justify-center gap-2.5">
          <div className="bg-primary text-primary-foreground grid size-9 place-items-center rounded-lg">
            <Radio className="size-5" />
          </div>
          <h1 className="text-lg font-semibold">On Air Record</h1>
        </div>

        <Card>
          <CardHeader>
            <CardTitle>Sign in</CardTitle>
            <CardDescription>This recorder needs an account to listen or make changes.</CardDescription>
          </CardHeader>
          <CardContent>
            <CredentialsForm submitLabel="Sign in" onSubmit={logIn} />
          </CardContent>
        </Card>

        <p className="text-muted-foreground text-center text-xs">
          Forgot your password? Ask an admin to set a new one, or reset it on the host with{' '}
          <code className="bg-muted rounded px-1 py-0.5">on-air-record auth reset-password</code>.
        </p>
      </div>
    </main>
  );
}
