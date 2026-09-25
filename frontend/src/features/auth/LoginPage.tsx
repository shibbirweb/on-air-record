/**
 * Shown instead of the app when accounts are on and nobody is signed in.
 *
 * Two steps for an account with two factor sign in: the password, then the code from the authenticator
 * app. The server remembers the right password in a short lived cookie, so a reload keeps the code step.
 *
 * Rendered outside the app shell on purpose, so no audio engine is built and no stream is opened until
 * somebody has signed in.
 */

import { Loader2, Radio } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';

import { ApiError } from '@/api/client';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { CredentialsForm } from '@/features/auth/CredentialsForm';
import { capitalise } from '@/lib/credentials';
import { useAuthStore } from '@/store/useAuthStore';

export function LoginPage() {
  const logIn = useAuthStore((state) => state.logIn);
  const pendingTwoFactor = useAuthStore((state) => state.pendingTwoFactor);

  return (
    <main className="bg-background grid min-h-screen place-items-center px-4 py-10">
      <div className="w-full max-w-sm space-y-6">
        <div className="flex items-center justify-center gap-2.5">
          <div className="bg-primary text-primary-foreground grid size-9 place-items-center rounded-lg">
            <Radio className="size-5" />
          </div>
          <h1 className="text-lg font-semibold">On Air Record</h1>
        </div>

        {pendingTwoFactor ? (
          <CodeStep />
        ) : (
          <>
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
          </>
        )}
      </div>
    </main>
  );
}

function CodeStep() {
  const id = useId();
  const verifyCode = useAuthStore((state) => state.verifyCode);
  const backToPassword = useAuthStore((state) => state.backToPassword);
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (code.trim() === '') {
      setError('Enter the code from your authenticator app.');
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await verifyCode(code.trim());
    } catch (cause) {
      setCode('');
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
      // An expired or exhausted sign in sends the page back to the password.
      if (cause instanceof ApiError && cause.message.includes('password again')) {
        void useAuthStore.getState().refresh();
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle>Enter your code</CardTitle>
          <CardDescription>
            Open your authenticator app and type the 6 digit code it shows for On Air Record.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
            <div className="space-y-2">
              <Label htmlFor={`${id}-code`}>Code</Label>
              <Input
                id={`${id}-code`}
                autoComplete="one-time-code"
                autoFocus
                spellCheck={false}
                placeholder="123456"
                className="tabular text-center text-lg tracking-widest"
                value={code}
                onChange={(event) => setCode(event.target.value)}
              />
            </div>

            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}

            <Button type="submit" className="w-full" disabled={busy}>
              {busy && <Loader2 className="animate-spin" />}
              Sign in
            </Button>
          </form>
        </CardContent>
      </Card>

      <div className="text-muted-foreground space-y-2 text-center text-xs">
        <p>No phone to hand? Type one of your recovery codes instead.</p>
        <Button variant="ghost" size="sm" onClick={() => void backToPassword()}>
          Use a different account
        </Button>
      </div>
    </>
  );
}
