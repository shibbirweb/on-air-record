/**
 * The Monitoring card: the Prometheus endpoint, a scrape config to paste, and the scrape token.
 *
 * None of this waits for Save. Making, rotating and revoking a token happen when confirmed, like adding an
 * account, because a token is shown only in the answer to making it. Rotating and revoking stop a running
 * scraper, so both ask first; making the first one breaks nothing and does not.
 */

import { Check, Copy, KeyRound, Loader2 } from 'lucide-react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { formatDateTime } from '@/lib/format';
import { metricsUrl, scrapeConfig, TOKEN_FILE } from '@/lib/scrapeConfig';
import { useAuthStore } from '@/store/useAuthStore';
import { useMetricsStore } from '@/store/useMetricsStore';

type Confirming = 'rotate' | 'revoke' | null;

export function MetricsSettings() {
  const mode = useAuthStore((state) => state.mode);
  const createdAtMs = useMetricsStore((state) => state.createdAtMs);
  const loaded = useMetricsStore((state) => state.loaded);
  const freshToken = useMetricsStore((state) => state.freshToken);
  const busy = useMetricsStore((state) => state.busy);
  const error = useMetricsStore((state) => state.error);
  const refresh = useMetricsStore((state) => state.refresh);
  const create = useMetricsStore((state) => state.create);
  const revoke = useMetricsStore((state) => state.revoke);
  const forgetFreshToken = useMetricsStore((state) => state.forgetFreshToken);
  const [confirming, setConfirming] = useState<Confirming>(null);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // The token exists nowhere else in the page, and nobody should find it on screen after moving on.
  useEffect(() => forgetFreshToken, [forgetFreshToken]);

  const origin = window.location.origin;
  const accounts = mode === 'accounts';
  // A scraper holding a token sends it in every mode, so the config shows it whenever one exists.
  const withToken = accounts || createdAtMs !== null;

  return (
    <div className="space-y-5">
      <div className="space-y-1 text-sm">
        <p>Prometheus can read the recorder&apos;s state, disk use and listeners from</p>
        <p className="font-mono text-xs break-all">{metricsUrl(origin)}</p>
        <p className="text-muted-foreground text-xs">
          The address is the one this page was opened on. Use one the Prometheus machine can reach.
        </p>
      </div>

      <div className="space-y-2">
        {loaded && createdAtMs === null && (
          <div className="flex flex-wrap items-start justify-between gap-3">
            <p className="text-sm">
              No scrape token.{' '}
              <span className="text-muted-foreground">
                {accounts
                  ? 'Prometheus cannot sign in, so it needs one to read the metrics while sign in is on.'
                  : 'It is not needed while anyone on the network can use the recorder, but a scraper that has one keeps working if sign in is turned on later.'}
              </span>
            </p>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => void create()}>
              {busy ? <Loader2 className="animate-spin" /> : <KeyRound />}
              Create token
            </Button>
          </div>
        )}

        {createdAtMs !== null && (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-sm">Scrape token created {formatDateTime(createdAtMs)}.</p>
            {confirming === null && (
              <div className="flex gap-2">
                <Button size="sm" variant="outline" disabled={busy} onClick={() => setConfirming('rotate')}>
                  Rotate
                </Button>
                <Button size="sm" variant="outline" disabled={busy} onClick={() => setConfirming('revoke')}>
                  Revoke
                </Button>
              </div>
            )}
          </div>
        )}

        {confirming !== null && (
          <div className="flex flex-wrap items-center justify-between gap-3 rounded-md border px-3 py-2">
            <p className="text-sm">
              {confirming === 'rotate'
                ? 'Make a new token? The current one stops working at once, so update Prometheus straight after.'
                : 'Revoke the token? Prometheus stops reading the metrics until it has a new one.'}
            </p>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="destructive"
                onClick={() => {
                  const action = confirming;
                  setConfirming(null);
                  void (action === 'rotate' ? create() : revoke());
                }}
              >
                {confirming === 'rotate' ? 'Make a new token' : 'Revoke the token'}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirming(null)}>
                Keep
              </Button>
            </div>
          </div>
        )}

        {freshToken && (
          <div className="space-y-2 rounded-md border border-amber-500/60 p-3">
            <p className="text-sm">
              Save this in <span className="font-mono text-xs">{TOKEN_FILE}</span> on the Prometheus machine. It
              will not be shown again.
            </p>
            <CopyableBlock text={freshToken} label="New scrape token" copyLabel="Copy the token" />
          </div>
        )}

        {error && <p className="text-destructive text-xs">{error}</p>}
      </div>

      <div className="space-y-2">
        <p className="text-sm">Add this to the Prometheus configuration:</p>
        <CopyableBlock
          text={scrapeConfig(origin, withToken)}
          label="Prometheus scrape config"
          copyLabel="Copy the scrape config"
        />
      </div>
    </div>
  );
}

/**
 * Text to select or copy. The clipboard exists only on https and localhost, so on a plain http address the
 * button is left out and the text is still there to select by hand.
 */
function CopyableBlock({ text, label, copyLabel }: { text: string; label: string; copyLabel: string }) {
  const [copied, setCopied] = useState(false);
  const canCopy = typeof navigator !== 'undefined' && Boolean(navigator.clipboard) && window.isSecureContext;

  return (
    // The label sits on a group, since a bare <pre> has no role that may carry one.
    <div role="group" aria-label={label} className="bg-muted relative rounded-md">
      <pre className="p-3 pr-12 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap select-all">
        {text}
      </pre>
      {canCopy && (
        <Button
          size="icon-sm"
          variant="ghost"
          className="absolute top-1.5 right-1.5"
          aria-label={copyLabel}
          onClick={() => {
            void navigator.clipboard.writeText(text).then(() => {
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
