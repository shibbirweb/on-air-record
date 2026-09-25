/**
 * Who can use the recorder: switch accounts on, or manage them once they are.
 *
 * Unlike the rest of the settings page, nothing here waits for the Save bar. Adding an account or
 * changing a role applies the moment it is confirmed, because a half saved account list is not a draft
 * anybody wants to review.
 */

import { KeyRound, Loader2, Trash2, UserPlus } from 'lucide-react';
import { useEffect, useId, useState, type FormEvent } from 'react';

import { ApiError } from '@/api/client';
import type { Role, User } from '@/api/types';
import { Badge } from '@/components/ui/badge';
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { CredentialsForm } from '@/features/auth/CredentialsForm';
import { capitalise, passwordProblem } from '@/lib/credentials';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useAuthStore } from '@/store/useAuthStore';

export function AccessSettings() {
  const mode = useAuthStore((state) => state.mode);
  return mode === 'accounts' ? <AccountsManager /> : <SwitchAccountsOn />;
}

function SwitchAccountsOn() {
  const setUp = useAuthStore((state) => state.setUp);
  const [open, setOpen] = useState(false);

  return (
    <div className="space-y-3">
      <p className="text-sm">
        No login is required. Anyone who can reach this page can listen, go back through the recordings,
        download them, and change these settings.
      </p>
      <Button onClick={() => setOpen(true)}>
        <KeyRound />
        Set up accounts
      </Button>

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create the admin account</DialogTitle>
            <DialogDescription>
              From then on everyone signs in. You become the admin and can add people who may only
              listen. The email is only what you sign in with; nothing is sent to it.
            </DialogDescription>
          </DialogHeader>
          <CredentialsForm
            submitLabel="Create and sign in"
            newPassword
            onSubmit={async (email, password) => {
              await setUp(email, password);
              setOpen(false);
            }}
          />
        </DialogContent>
      </Dialog>
    </div>
  );
}

function AccountsManager() {
  const me = useAuthStore((state) => state.user);
  const users = useAccountsStore((state) => state.users);
  const loading = useAccountsStore((state) => state.loading);
  const error = useAccountsStore((state) => state.error);
  const refresh = useAccountsStore((state) => state.refresh);
  const create = useAccountsStore((state) => state.create);
  const [adding, setAdding] = useState(false);
  const [newRole, setNewRole] = useState<Role>('listener');
  // Remounting the form after each account clears its fields for the next one.
  const [formKey, setFormKey] = useState(0);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return (
    <div className="space-y-4">
      <p className="text-muted-foreground text-sm">
        Admins control everything, including this list. Listeners can listen, scrub back and export, and
        change nothing. Changes here apply straight away.
      </p>

      {error && (
        <p role="alert" className="text-destructive text-sm">
          {capitalise(error)}
        </p>
      )}

      <ul className="divide-y rounded-lg border">
        {users.map((user) => (
          <AccountRow key={user.id} user={user} isMe={user.id === me?.id} />
        ))}
        {users.length === 0 && (
          <li className="text-muted-foreground px-3 py-3 text-sm">
            {loading ? 'Loading accounts...' : 'No accounts.'}
          </li>
        )}
      </ul>

      <Button variant="outline" onClick={() => setAdding(true)}>
        <UserPlus />
        Add an account
      </Button>

      <Dialog open={adding} onOpenChange={setAdding}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Add an account</DialogTitle>
            <DialogDescription>
              Give the person their email and this password. They can change the password after signing
              in.
            </DialogDescription>
          </DialogHeader>
          <CredentialsForm
            key={formKey}
            submitLabel="Add account"
            newPassword
            onSubmit={async (email, password) => {
              await create(email, password, newRole);
              setFormKey((key) => key + 1);
              setAdding(false);
            }}
          >
            <div className="space-y-2">
              <Label>Role</Label>
              <RolePicker value={newRole} onChange={setNewRole} />
            </div>
          </CredentialsForm>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function AccountRow({ user, isMe }: { user: User; isMe: boolean }) {
  const updateRole = useAccountsStore((state) => state.updateRole);
  const remove = useAccountsStore((state) => state.remove);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [settingPassword, setSettingPassword] = useState(false);

  return (
    <li className="flex flex-wrap items-center gap-2 px-3 py-2">
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm">
          {user.email}
          {isMe && (
            <Badge variant="outline" className="ml-2 font-normal">
              you
            </Badge>
          )}
        </p>
      </div>

      {/* Your own role and account are changed by another admin, so nobody locks themselves out by
          accident halfway through a list. */}
      {isMe ? (
        <Badge variant="secondary" className="font-normal">
          {user.role === 'admin' ? 'Admin' : 'Listener'}
        </Badge>
      ) : (
        <>
          <div className="w-32">
            <RolePicker value={user.role} onChange={(role) => void updateRole(user.id, role)} />
          </div>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label={`Set a new password for ${user.email}`}
            onClick={() => setSettingPassword(true)}
          >
            <KeyRound />
          </Button>
          {confirmingRemove ? (
            <>
              <Button size="sm" variant="destructive" onClick={() => void remove(user.id)}>
                Remove
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirmingRemove(false)}>
                Keep
              </Button>
            </>
          ) : (
            <Button
              size="icon-sm"
              variant="ghost"
              aria-label={`Remove ${user.email}`}
              onClick={() => setConfirmingRemove(true)}
            >
              <Trash2 />
            </Button>
          )}
        </>
      )}

      <SetPasswordDialog user={user} open={settingPassword} onOpenChange={setSettingPassword} />
    </li>
  );
}

function RolePicker({ value, onChange }: { value: Role; onChange: (role: Role) => void }) {
  return (
    <Select value={value} onValueChange={(next) => onChange(next as Role)}>
      <SelectTrigger className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="listener">Listener</SelectItem>
        <SelectItem value="admin">Admin</SelectItem>
      </SelectContent>
    </Select>
  );
}

type SetPasswordDialogProps = {
  user: User;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

function SetPasswordDialog({ user, open, onOpenChange }: SetPasswordDialogProps) {
  const id = useId();
  const setPassword = useAccountsStore((state) => state.setPassword);
  const [password, setPasswordValue] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const close = () => {
    setPasswordValue('');
    setConfirmation('');
    setError(null);
    onOpenChange(false);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const problem = passwordProblem(password, confirmation);
    if (problem) {
      setError(problem);
      return;
    }
    setBusy(true);
    try {
      await setPassword(user.id, password);
      close();
    } catch (cause) {
      setError(cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(isOpen) => (isOpen ? onOpenChange(true) : close())}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New password for {user.email}</DialogTitle>
          <DialogDescription>
            They are signed out everywhere and sign in again with this password.
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
          <div className="space-y-2">
            <Label htmlFor={`${id}-password`}>New password</Label>
            <Input
              id={`${id}-password`}
              type="password"
              autoComplete="new-password"
              autoFocus
              value={password}
              onChange={(event) => setPasswordValue(event.target.value)}
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
          <Separator />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy}>
              {busy && <Loader2 className="animate-spin" />}
              Set password
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
