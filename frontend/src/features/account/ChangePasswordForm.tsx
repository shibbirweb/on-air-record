/**
 * Change your own password, on the account settings page. Every other browser signed in to the account is
 * signed out, which the form says, because that is the reason to change a password that may have leaked.
 */

import { Check, Loader2 } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';

import { ApiError } from '@/api/client';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { capitalise, passwordProblem } from '@/lib/credentials';
import { useAuthStore } from '@/store/useAuthStore';

export function ChangePasswordForm() {
  const id = useId();
  const changePassword = useAuthStore((state) => state.changePassword);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const problem = current === '' ? 'Enter your current password.' : passwordProblem(next, confirmation);
    if (problem) {
      setError(problem);
      return;
    }

    setBusy(true);
    setError(null);
    setDone(false);
    try {
      await changePassword(current, next);
      // Clear the fields so the new password is not left sitting in the page.
      setCurrent('');
      setNext('');
      setConfirmation('');
      setDone(true);
    } catch (cause) {
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="max-w-sm space-y-4" onSubmit={(event) => void submit(event)} noValidate>
      <div className="space-y-2">
        <Label htmlFor={`${id}-current`}>Current password</Label>
        <Input
          id={`${id}-current`}
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(event) => setCurrent(event.target.value)}
        />
      </div>
      <div className="space-y-2">
        <Label htmlFor={`${id}-next`}>New password</Label>
        <Input
          id={`${id}-next`}
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(event) => setNext(event.target.value)}
        />
        <p className="text-muted-foreground text-xs">At least 8 characters. A short sentence works well.</p>
      </div>
      <div className="space-y-2">
        <Label htmlFor={`${id}-confirm`}>New password again</Label>
        <Input
          id={`${id}-confirm`}
          type="password"
          autoComplete="new-password"
          value={confirmation}
          onChange={(event) => setConfirmation(event.target.value)}
        />
      </div>

      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}
      {done && (
        <p role="status" className="flex items-center gap-1.5 text-sm">
          <Check className="text-primary size-4" />
          Your password has been changed. Other browsers have been signed out.
        </p>
      )}

      <Button type="submit" disabled={busy}>
        {busy && <Loader2 className="animate-spin" />}
        Change password
      </Button>
    </form>
  );
}
