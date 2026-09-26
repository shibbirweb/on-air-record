/**
 * What's new since the running version, and how to install it.
 *
 * Every release between this one and the newest is listed, not just the newest, because skipping two
 * releases means two sets of changes. The steps below them depend on how this copy was installed, which
 * the service reports, so the commands can be copied as they are.
 */

import { Check, Copy, ExternalLink } from 'lucide-react';
import { useState } from 'react';

import type { UpdateStatus } from '@/api/types';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Separator } from '@/components/ui/separator';
import { ReleaseNotes } from '@/features/updates/ReleaseNotes';
import { updatePlan } from '@/lib/updateSteps';

const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { year: 'numeric', month: 'short', day: 'numeric' });

function CopyableCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  // The clipboard API exists only on https and localhost. On a plain http address the command is still
  // there to select by hand.
  const canCopy = typeof navigator !== 'undefined' && Boolean(navigator.clipboard) && window.isSecureContext;

  return (
    <div className="bg-muted relative rounded-md">
      {/* Wrapped rather than scrolled, so the whole command is readable and nothing hides under the copy
          button. Wrapping only changes how it looks: the text copied is the same single line. */}
      <pre className="p-3 pr-12 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap select-all">
        {command}
      </pre>
      {canCopy && (
        <Button
          size="icon-sm"
          variant="ghost"
          className="absolute top-1.5 right-1.5"
          aria-label="Copy the command"
          onClick={() => {
            void navigator.clipboard.writeText(command).then(() => {
              setCopied(true);
              window.setTimeout(() => setCopied(false), 1500);
            });
          }}
        >
          {copied ? <Check /> : <Copy />}
        </Button>
      )}
    </div>
  );
}

type WhatsNewDialogProps = {
  status: UpdateStatus;
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

export function WhatsNewDialog({ status, open, onOpenChange }: WhatsNewDialogProps) {
  const newest = status.available;
  if (!newest) {
    return null;
  }
  const plan = updatePlan(status, newest);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>What&apos;s new in {newest.version}</DialogTitle>
          <DialogDescription>
            You have {status.currentVersion}.{' '}
            {status.releases.length > 1
              ? `${status.releases.length} releases have come out since, newest first.`
              : 'Here is what changed.'}
          </DialogDescription>
        </DialogHeader>

        {/* The dialog is a grid, whose items refuse to shrink below their content without min-w-0: one
            long URL or command would otherwise widen the whole dialog past its frame. */}
        <div className="min-w-0 space-y-5">
          {status.releases.map((release) => (
            <section key={release.tag} className="min-w-0 space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="font-semibold">{release.version}</h3>
                {release.prerelease && (
                  <Badge variant="secondary" className="font-normal">
                    Beta
                  </Badge>
                )}
                {release.publishedAtMs !== null && (
                  <span className="text-muted-foreground text-xs">
                    {DATE_FORMAT.format(new Date(release.publishedAtMs))}
                  </span>
                )}
                <a
                  href={release.url}
                  target="_blank"
                  rel="noreferrer noopener"
                  className="text-muted-foreground ml-auto inline-flex items-center gap-1 text-xs underline"
                >
                  Release page <ExternalLink className="size-3" />
                </a>
              </div>
              <ReleaseNotes markdown={release.notes} />
            </section>
          ))}
        </div>

        <Separator />

        <section className="min-w-0 space-y-3">
          <h3 className="font-semibold">How to update</h3>
          <p className="text-muted-foreground text-sm">
            Updating restarts the service, so a few seconds are not recorded. Settings, accounts and
            recordings are kept.
          </p>
          <ol className="list-decimal space-y-3 pl-5 text-sm">
            {plan.steps.map((step, index) => (
              <li key={index} className="min-w-0 space-y-2">
                <p>{step.text}</p>
                {step.command && <CopyableCommand command={step.command} />}
              </li>
            ))}
          </ol>
          {plan.download && (
            <p className="text-muted-foreground text-xs">
              The download for this machine is{' '}
              <a href={plan.download.url} className="underline">
                {plan.download.name}
              </a>
              .
            </p>
          )}
          {plan.note && <p className="text-muted-foreground text-xs">{plan.note}</p>}
        </section>
      </DialogContent>
    </Dialog>
  );
}
