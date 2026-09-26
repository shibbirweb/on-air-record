/**
 * The signed in account, in the header: who you are, the way to your account settings, and sign out.
 *
 * Renders nothing without accounts, where nobody is signed in.
 */

import { LogOut, UserCog, UserRound } from 'lucide-react';
import { useState } from 'react';
import { Link } from 'react-router';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Separator } from '@/components/ui/separator';
import { useAuthStore } from '@/store/useAuthStore';

export function UserMenu() {
  const user = useAuthStore((state) => state.user);
  const logOut = useAuthStore((state) => state.logOut);
  const [open, setOpen] = useState(false);

  if (!user) {
    return null;
  }

  return (
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
          <Button asChild variant="ghost" size="sm" className="w-full justify-start">
            <Link to="/account" onClick={() => setOpen(false)}>
              <UserCog />
              Account settings
            </Link>
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
  );
}
