/**
 * The activity log, on its own page: who signed in, changed something or listened, when, and from where.
 *
 * Admins only, like the settings page; the server refuses anybody else whatever this page does. Nothing
 * here edits or deletes an entry: the log is only ever trimmed by its retention window, so it cannot be
 * tidied from the page it is read on.
 */

import { Loader2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { Navigate } from 'react-router';

import type { ActivityEntry, ActivityGroup } from '@/api/types';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group';
import { describeActor, describeEvent, groupOf } from '@/lib/activityText';
import { formatDateTime } from '@/lib/format';
import { useAccountsStore } from '@/store/useAccountsStore';
import { useActivityStore } from '@/store/useActivityStore';
import { useAuthStore, useCanAdminister } from '@/store/useAuthStore';

const GROUPS: { value: ActivityGroup | 'all'; label: string }[] = [
  { value: 'all', label: 'Everything' },
  { value: 'access', label: 'Sign ins' },
  { value: 'accounts', label: 'Accounts' },
  { value: 'listening', label: 'Listening' },
  { value: 'recorder', label: 'Recorder' },
];

const GROUP_LABELS: Record<ActivityGroup, string> = {
  access: 'Sign in',
  accounts: 'Account',
  listening: 'Listening',
  recorder: 'Recorder',
};

export function ActivityPage() {
  const mayAdminister = useCanAdminister();
  const mode = useAuthStore((state) => state.mode);
  const entries = useActivityStore((state) => state.entries);
  const filter = useActivityStore((state) => state.filter);
  const loaded = useActivityStore((state) => state.loaded);
  const loadingMore = useActivityStore((state) => state.loadingMore);
  const hasMore = useActivityStore((state) => state.hasMore);
  const error = useActivityStore((state) => state.error);
  const refresh = useActivityStore((state) => state.refresh);
  const loadMore = useActivityStore((state) => state.loadMore);
  const setFilter = useActivityStore((state) => state.setFilter);
  const users = useAccountsStore((state) => state.users);
  const refreshUsers = useAccountsStore((state) => state.refresh);
  const [email, setEmail] = useState(filter.email);

  useEffect(() => {
    if (mayAdminister) {
      void refresh();
    }
  }, [mayAdminister, refresh]);

  // Only for the suggestions in the person box. On an open recorder there are no accounts to list.
  useEffect(() => {
    if (mayAdminister && mode === 'accounts') {
      void refreshUsers();
    }
  }, [mayAdminister, mode, refreshUsers]);

  // The link is hidden from listeners, but a typed or bookmarked URL still lands here.
  if (!mayAdminister) {
    return <Navigate to="/" replace />;
  }

  const applyEmail = (event: FormEvent) => {
    event.preventDefault();
    void setFilter({ email: email.trim() });
  };

  const filtered = filter.email !== '' || filter.group !== null;

  return (
    <main className="mx-auto max-w-3xl space-y-4 px-4 py-6">
      <div className="space-y-1">
        <h2 className="text-xl font-semibold">Activity</h2>
        <p className="text-muted-foreground text-sm">
          Who signed in, changed something or listened, when, and from where. Entries are kept for as long as
          Settings, Activity log says.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Show</CardTitle>
          <CardDescription>Narrow the log to one kind of event, one person, or both.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <RadioGroup
            aria-label="Kind of event"
            className="flex flex-wrap gap-x-4 gap-y-2"
            value={filter.group ?? 'all'}
            onValueChange={(value) => void setFilter({ group: value === 'all' ? null : (value as ActivityGroup) })}
          >
            {GROUPS.map((group) => (
              <div key={group.value} className="flex items-center gap-2">
                <RadioGroupItem value={group.value} id={`activity-group-${group.value}`} />
                <Label htmlFor={`activity-group-${group.value}`} className="font-normal">
                  {group.label}
                </Label>
              </div>
            ))}
          </RadioGroup>

          <form className="flex flex-wrap items-end gap-2" onSubmit={applyEmail}>
            <div className="min-w-0 flex-1 space-y-1">
              <Label htmlFor="activity-email">Only this person</Label>
              <Input
                id="activity-email"
                type="search"
                list="activity-accounts"
                placeholder="Everyone"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
              />
              <datalist id="activity-accounts">
                {users.map((user) => (
                  <option key={user.id} value={user.email} />
                ))}
              </datalist>
            </div>
            <Button type="submit" variant="outline">
              Show
            </Button>
          </form>
        </CardContent>
      </Card>

      {error && <p className="text-destructive text-sm">{error}</p>}

      {!loaded ? (
        <p className="text-muted-foreground text-sm">Loading the activity log...</p>
      ) : entries.length === 0 ? (
        <p className="text-muted-foreground text-sm">
          {filtered ? 'Nothing matches these filters.' : 'Nothing has been logged yet.'}
        </p>
      ) : (
        <ol className="divide-y rounded-lg border">
          {entries.map((entry) => (
            <EntryRow key={entry.id} entry={entry} />
          ))}
        </ol>
      )}

      {loaded && entries.length > 0 && (
        <div className="flex justify-center">
          {hasMore ? (
            <Button variant="outline" disabled={loadingMore} onClick={() => void loadMore()}>
              {loadingMore && <Loader2 className="animate-spin" />}
              Load older entries
            </Button>
          ) : (
            <p className="text-muted-foreground text-xs">That is everything kept.</p>
          )}
        </div>
      )}
    </main>
  );
}

function EntryRow({ entry }: { entry: ActivityEntry }) {
  return (
    <li className="space-y-1 px-3 py-2">
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <p className="min-w-0 text-sm">
          <span className="font-medium break-all">{describeActor(entry.actor)}</span>{' '}
          <span className="text-muted-foreground">&middot;</span> {describeEvent(entry.event)}
        </p>
        <Badge variant="outline" className="font-normal">
          {GROUP_LABELS[groupOf(entry.event.kind)]}
        </Badge>
      </div>
      <p className="text-muted-foreground flex flex-wrap gap-x-3 text-xs tabular">
        <span>{formatDateTime(entry.atMs)}</span>
        {entry.address && <span>{entry.address}</span>}
        {entry.userAgent && (
          <span className="max-w-full truncate sm:max-w-80" title={entry.userAgent}>
            {entry.userAgent}
          </span>
        )}
      </p>
    </li>
  );
}
