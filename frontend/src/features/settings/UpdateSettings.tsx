/**
 * The Updates card: the running version, whether a newer one is out, Check now, and the switch for the
 * automatic check, which is the one request the service makes to the internet.
 *
 * The switch is part of the settings draft and waits for Save like everything else on the page. Check now
 * does not: pressing it is the request.
 */

import { Loader2, RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';
import { WhatsNewDialog } from '@/features/updates/WhatsNewDialog';
import { formatDateTime } from '@/lib/format';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useUpdateStore } from '@/store/useUpdateStore';

export function UpdateSettings() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);
  const status = useUpdateStore((state) => state.status);
  const checking = useUpdateStore((state) => state.checking);
  const requestError = useUpdateStore((state) => state.requestError);
  const refresh = useUpdateStore((state) => state.refresh);
  const checkNow = useUpdateStore((state) => state.checkNow);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const automatic = draft.checkForUpdates ?? settings?.checkForUpdates ?? true;
  const channel = status?.channel === 'beta' ? 'beta and stable releases' : 'stable releases';

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1 text-sm">
          <p>
            Version <span className="font-medium tabular">{status?.currentVersion ?? '...'}</span>, following{' '}
            {channel}.
          </p>
          {status?.available ? (
            <p className="font-medium text-amber-700 dark:text-amber-400">
              {status.available.version} is available.
            </p>
          ) : status?.checkedAtMs ? (
            <p className="text-muted-foreground">Up to date.</p>
          ) : null}
          <p className="text-muted-foreground text-xs">
            {status?.checkedAtMs ? `Last checked ${formatDateTime(status.checkedAtMs)}.` : 'Not checked yet.'}
          </p>
          {status?.error && (
            <p className="text-destructive text-xs">The last check failed: {status.error}.</p>
          )}
          {requestError && <p className="text-destructive text-xs">{requestError}</p>}
        </div>
        <div className="flex gap-2">
          {status?.available && (
            <Button size="sm" onClick={() => setOpen(true)}>
              What&apos;s new
            </Button>
          )}
          <Button size="sm" variant="outline" disabled={checking} onClick={() => void checkNow()}>
            {checking ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            Check now
          </Button>
        </div>
      </div>

      <div className="flex items-center justify-between gap-4">
        <div className="space-y-1">
          <Label htmlFor="check-for-updates">Check for updates automatically</Label>
          <p className="text-muted-foreground text-xs">
            Asks GitHub every few hours whether a newer release is out, and tells admins when one is. It never
            installs anything. This is the only request the recorder makes to the internet; switch it off for
            a machine that should make none.
          </p>
        </div>
        <Switch
          id="check-for-updates"
          checked={automatic}
          onCheckedChange={(checked) => edit({ checkForUpdates: checked })}
        />
      </div>

      {status && <WhatsNewDialog status={status} open={open} onOpenChange={setOpen} />}
    </div>
  );
}
