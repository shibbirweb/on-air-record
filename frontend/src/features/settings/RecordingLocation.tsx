/**
 * Where segment files are written.
 *
 * Two things make an empty text box for a filesystem path unusable: there is no clue what shape the value
 * should take, and there is no way to find out a path is wrong until it is too late. So the field shows
 * worked examples in the server's own path convention, and Test asks the server what would actually
 * happen without changing anything on disk.
 *
 * A change applies to the next recording session, never the current one. Moving mid session would scatter
 * one recording across two roots, and recordings already on disk are left exactly where they are.
 */

import { CheckCircle2, FolderOpen, Info, Loader2, RotateCcw, XCircle } from 'lucide-react';
import { useState } from 'react';

import { api } from '@/api/client';
import type { DirectoryTest } from '@/api/types';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { pathExamples } from '@/lib/paths';
import { cn } from '@/lib/utils';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';

export function RecordingLocation() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);
  const capturing = useStatusStore((state) => state.status?.capture.state === 'recording');

  const [testing, setTesting] = useState(false);
  /**
   * The result is kept alongside the path it was produced for, so editing the box invalidates a stale
   * verdict without an effect watching the value.
   */
  const [probe, setProbe] = useState<{ path: string; result: DirectoryTest } | null>(null);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading...</p>;
  }

  const pending = { ...settings, ...draft };
  const value = pending.recordingsDir ?? '';
  const examples = pathExamples(settings.effectiveRecordingsDir);
  const result = probe && probe.path === value ? probe.result : null;

  const stage = (next: string) => {
    const trimmed = next.trim();
    edit({ recordingsDir: trimmed === '' ? null : trimmed });
  };

  const test = async () => {
    setTesting(true);
    try {
      const outcome = await api.testRecordingsDir(value === '' ? null : value);
      setProbe({ path: value, result: outcome });
    } catch (cause) {
      setProbe({
        path: value,
        result: {
          ok: false,
          resolvedPath: value,
          exists: false,
          willCreate: false,
          readable: false,
          writable: false,
          message: cause instanceof Error ? cause.message : 'the service could not be reached',
        },
      });
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="recordings-dir">Recording directory</Label>

        <div className="flex gap-2">
          <Input
            id="recordings-dir"
            value={value}
            spellCheck={false}
            placeholder={settings.effectiveRecordingsDir}
            className="font-mono text-xs"
            onChange={(event) => stage(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                void test();
              }
            }}
          />
          <Button variant="outline" disabled={testing} onClick={() => void test()}>
            {testing ? <Loader2 className="animate-spin" /> : null}
            {testing ? 'Testing' : 'Test'}
          </Button>
        </div>

        <div className="text-muted-foreground space-y-1 text-xs">
          <p>
            An absolute path on the machine running the service, for example{' '}
            <code className="bg-muted rounded px-1 py-0.5 font-mono">{examples.absolute}</code>. A path
            with no {examples.style === 'windows' ? 'drive letter' : 'leading slash'} is taken from the
            data directory, so{' '}
            <code className="bg-muted rounded px-1 py-0.5 font-mono">{examples.relative}</code> would sit
            beside the database.
          </p>
          <p>Leave it empty to use the default location shown below.</p>
        </div>
      </div>

      {result && (
        <div
          className={cn(
            'flex items-start gap-2 rounded-lg border p-3 text-xs',
            result.ok
              ? 'border-primary/40 bg-primary/5'
              : 'border-destructive/40 bg-destructive/5 text-destructive',
          )}
        >
          {result.ok ? (
            <CheckCircle2 className="text-primary mt-0.5 size-4 shrink-0" />
          ) : (
            <XCircle className="mt-0.5 size-4 shrink-0" />
          )}
          <div className="min-w-0 space-y-1">
            <p className={result.ok ? 'text-foreground' : undefined}>{result.message}</p>
            <p className="text-muted-foreground truncate font-mono" title={result.resolvedPath}>
              {result.resolvedPath}
            </p>
          </div>
        </div>
      )}

      <div className="bg-muted/40 space-y-2 rounded-lg border p-3">
        <div className="flex items-start gap-2">
          <FolderOpen className="text-muted-foreground mt-0.5 size-3.5 shrink-0" />
          <div className="min-w-0 space-y-0.5">
            <p className="text-muted-foreground text-xs">Currently writing to</p>
            <p className="truncate font-mono text-xs" title={settings.effectiveRecordingsDir}>
              {settings.effectiveRecordingsDir}
            </p>
          </div>
        </div>

        {pending.recordingsDir !== null && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7"
            onClick={() => edit({ recordingsDir: null })}
          >
            <RotateCcw />
            Use the default location
          </Button>
        )}
      </div>

      <p className="text-muted-foreground flex items-start gap-1.5 text-xs">
        <Info className="mt-0.5 size-3.5 shrink-0" />
        {capturing
          ? 'A change takes effect on the next recording session. Stop and start the recorder to move sooner. Existing recordings stay where they are and remain playable.'
          : 'A change takes effect the next time recording starts. Existing recordings stay where they are and remain playable.'}
      </p>
    </div>
  );
}
