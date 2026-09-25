/**
 * Change your own password. Every other browser signed in to the account is signed out, which the dialog
 * says, because that is the reason to change a password that may have leaked.
 */

import { Loader2 } from 'lucide-react';
import { useId, useState, type FormEvent } from 'react';

import { ApiError } from '@/api/client';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { capitalise, passwordProblem } from '@/lib/credentials';
import { useAuthStore } from '@/store/useAuthStore';

type ChangePasswordDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function ChangePasswordDialog({ open, onOpenChange }: ChangePasswordDialogProps) {
  const id = useId();
  const changePassword = useAuthStore((state) => state.changePassword);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState(false);

  const reset = () => {
    setCurrent('');
    setNext('');
    setConfirmation('');
    setError(null);
    setDone(false);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const problem = current === '' ? 'Enter your current password.' : passwordProblem(next, confirmation);
    if (problem) {
      setError(problem);
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await changePassword(current, next);
      setDone(true);
    } catch (cause) {
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(isOpen) => {
        if (!isOpen) {
          reset();
        }
        onOpenChange(isOpen);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Change your password</DialogTitle>
          <DialogDescription>
            Other browsers signed in to this account will be signed out. This one stays signed in.
          </DialogDescription>
        </DialogHeader>

        {done ? (
          <>
            <p className="text-sm">Your password has been changed.</p>
            <DialogFooter>
              <Button onClick={() => onOpenChange(false)}>Done</Button>
            </DialogFooter>
          </>
        ) : (
          <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
            <div className="space-y-2">
              <Label htmlFor={`${id}-current`}>Current password</Label>
              <Input
                id={`${id}-current`}
                type="password"
                autoComplete="current-password"
                autoFocus
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

            <DialogFooter>
              <Button type="submit" disabled={busy}>
                {busy && <Loader2 className="animate-spin" />}
                Change password
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
