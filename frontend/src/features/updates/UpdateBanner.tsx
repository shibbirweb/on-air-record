/**
 * The strip under the header saying a newer release is out. Admins only: nobody else can act on it.
 *
 * "Later" hides it for that version in this browser; the next release brings it back.
 */

import { ArrowUpCircle, X } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { WhatsNewDialog } from '@/features/updates/WhatsNewDialog';
import { shouldShowBanner, useUpdateStore } from '@/store/useUpdateStore';

export function UpdateBanner() {
  const status = useUpdateStore((state) => state.status);
  const dismissed = useUpdateStore((state) => state.dismissed);
  const dismiss = useUpdateStore((state) => state.dismiss);
  const [open, setOpen] = useState(false);

  if (!status?.available || !shouldShowBanner(status, dismissed)) {
    return null;
  }
  const newest = status.available;

  return (
    <div className="border-b bg-amber-50 dark:bg-amber-950/40">
      <div className="mx-auto flex max-w-[1600px] flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2 text-sm">
        <ArrowUpCircle className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
        <p className="min-w-0 flex-1">
          <span className="font-medium">On Air Record {newest.version} is available.</span>{' '}
          <span className="text-muted-foreground">You have {status.currentVersion}.</span>
        </p>
        <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
          What&apos;s new and how to update
        </Button>
        <Button
          size="icon-sm"
          variant="ghost"
          aria-label={`Hide until the release after ${newest.version}`}
          title="Later"
          onClick={() => dismiss(newest.version)}
        >
          <X />
        </Button>
      </div>
      <WhatsNewDialog status={status} open={open} onOpenChange={setOpen} />
    </div>
  );
}
