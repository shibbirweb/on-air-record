/**
 * The signed in account, in the header: who you are, change your password, sign out.
 *
 * Renders nothing without accounts, where nobody is signed in.
 */

import { KeyRound, LogOut, UserRound } from 'lucide-react';
import { useState } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { ChangePasswordDialog } from '@/features/auth/ChangePasswordDialog';
import { useAuthStore } from '@/store/useAuthStore';

export function UserMenu() {
  const user = useAuthStore((state) => state.user);
  const logOut = useAuthStore((state) => state.logOut);
  const [open, setOpen] = useState(false);
  const [changingPassword, setChangingPassword] = useState(false);

  if (!user) {
    return null;
  }

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="icon" aria-label={`Account: ${user.email}`}>
            <UserRound />
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-64 p-0">
          <div className="space-y-1 px-3 py-3">
            <p className="truncate text-sm font-medium">{user.email}</p>
            <Badge variant="secondary" className="font-normal">
              {user.role === 'admin' ? 'Admin' : 'Listener'}
            </Badge>
          </div>
          <Separator />
          <div className="p-1">
            <Button
              variant="ghost"
              size="sm"
              className="w-full justify-start"
              onClick={() => {
                setOpen(false);
                setChangingPassword(true);
              }}
            >
              <KeyRound />
              Change password
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="w-full justify-start"
              onClick={() => void logOut()}
            >
              <LogOut />
              Sign out
            </Button>
          </div>
        </PopoverContent>
      </Popover>

      <ChangePasswordDialog open={changingPassword} onOpenChange={setChangingPassword} />
    </>
  );
}
