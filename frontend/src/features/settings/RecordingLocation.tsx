/**
 * Where segment files are written.
 *
 * The server validates the path before accepting it, by creating the directory and writing a probe file.
 * A directory that merely exists is not enough: a read only mount or a permissions problem would
 * otherwise show up as a stream of failed segments with nothing here to explain why.
 *
 * A change applies to the next recording session, never the current one. Moving mid session would scatter
 * one recording across two roots, and recordings already on disk are left exactly where they are.
 */

import { Check, FolderOpen, Info, RotateCcw } from 'lucide-react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';

export function RecordingLocation() {
  const settings = useSettingsStore((state) => state.settings);
  const saving = useSettingsStore((state) => state.saving);
  const error = useSettingsStore((state) => state.error);
  const update = useSettingsStore((state) => state.update);
  const capturing = useStatusStore((state) => state.status?.capture.state === 'recording');

  /**
   * `null` means "show whatever the server has". Only a keystroke puts a value here, and applying clears
   * it again, so the box follows the stored setting without an effect syncing the two.
   */
  const [edit, setEdit] = useState<string | null>(null);
  const [applied, setApplied] = useState(false);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading...</p>;
  }

  const stored = settings.recordingsDir ?? '';
  const draft = edit ?? stored;
  const trimmed = draft.trim();
  const dirty = trimmed !== stored;

  const apply = async (value: string | null) => {
    setApplied(false);
    await update({ recordingsDir: value });
    // The store keeps the previous value on failure, so success is "the server took what we sent".
    const saved = useSettingsStore.getState().settings?.recordingsDir ?? null;
    if (saved === value) {
      setEdit(null);
      setApplied(true);
      window.setTimeout(() => setApplied(false), 2500);
    }
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="recordings-dir">Recording directory</Label>
        <div className="flex gap-2">
          <Input
            id="recordings-dir"
            value={draft}
            spellCheck={false}
            placeholder="Leave empty to use the default location"
            className="font-mono text-xs"
            onChange={(event) => setEdit(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' && dirty) {
                void apply(trimmed === '' ? null : trimmed);
              }
            }}
          />
          <Button
            variant={dirty ? 'default' : 'outline'}
            disabled={!dirty || saving}
            onClick={() => void apply(trimmed === '' ? null : trimmed)}
          >
            {applied ? <Check /> : null}
            {applied ? 'Saved' : 'Apply'}
          </Button>
        </div>
        <p className="text-muted-foreground text-xs">
          An absolute path on the machine running the service. A relative path is taken from the data
          directory.
        </p>
      </div>

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

        {settings.recordingsDir !== null && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7"
            disabled={saving}
            onClick={() => void apply(null)}
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

      {error && <p className="text-destructive text-xs">{error}</p>}
    </div>
  );
}
