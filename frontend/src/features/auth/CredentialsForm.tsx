/**
 * Email and password, shared by the first run setup, the login page, and adding an account.
 *
 * `newPassword` switches on the confirmation field and tells the browser's password manager to offer a
 * generated password instead of filling a saved one.
 */

import { Loader2 } from 'lucide-react';
import { useId, useState, type FormEvent, type ReactNode } from 'react';

import { ApiError } from '@/api/client';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { capitalise, emailProblem, passwordProblem } from '@/lib/credentials';

type CredentialsFormProps = {
  submitLabel: string;
  newPassword?: boolean;
  onSubmit: (email: string, password: string) => Promise<void>;
  /** Extra fields between the password and the button, such as a role picker. */
  children?: ReactNode;
};

export function CredentialsForm({
  submitLabel,
  newPassword = false,
  onSubmit,
  children,
}: CredentialsFormProps) {
  const id = useId();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();

    const problem =
      emailProblem(email) ??
      (newPassword ? passwordProblem(password, confirmation) : password === '' ? 'Enter your password.' : null);
    if (problem) {
      setError(problem);
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await onSubmit(email.trim(), password);
    } catch (cause) {
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
      <div className="space-y-2">
        <Label htmlFor={`${id}-email`}>Email</Label>
        <Input
          id={`${id}-email`}
          type="email"
          autoComplete={newPassword ? 'email' : 'username'}
          autoFocus
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor={`${id}-password`}>Password</Label>
        <Input
          id={`${id}-password`}
          type="password"
          autoComplete={newPassword ? 'new-password' : 'current-password'}
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
        {newPassword && (
          <p className="text-muted-foreground text-xs">At least 8 characters. A short sentence works well.</p>
        )}
      </div>

      {newPassword && (
        <div className="space-y-2">
          <Label htmlFor={`${id}-confirm`}>Password again</Label>
          <Input
            id={`${id}-confirm`}
            type="password"
            autoComplete="new-password"
            value={confirmation}
            onChange={(event) => setConfirmation(event.target.value)}
          />
        </div>
      )}

      {children}

      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}

      <Button type="submit" className="w-full" disabled={busy}>
        {busy && <Loader2 className="animate-spin" />}
        {submitLabel}
      </Button>
    </form>
  );
}

