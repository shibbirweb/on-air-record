/**
 * Recording quality, chosen by bit rate.
 *
 * Segments are uncompressed PCM, so the bit rate is not a codec setting: it is arithmetic.
 * `sample rate * 16 bits * 1 channel` is exactly what lands on disk, which makes every option here a
 * precise storage multiplier rather than an estimate. Halving the rate halves the disk.
 *
 * The ladder is sample rates rather than a compressor's bit rate dial because that is the only lever the
 * format has. Real codec compression would give far more for the quality, and is tracked separately.
 */

import { AudioLines } from 'lucide-react';

import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { formatBytes, pcmBitRateKbps, pcmBytesPerHour } from '@/lib/format';
import { useSettingsStore } from '@/store/useSettingsStore';
import { useStatusStore } from '@/store/useStatusStore';

/** Sentinel for "follow the device", since a select item cannot carry a null value. */
const DEVICE_RATE = '__device__';

/** Must match `SUPPORTED_SAMPLE_RATES` in `backend/src/models/settings.rs`. */
const RATES = [
  { hz: 48_000, label: 'Full quality', note: 'music and detail' },
  { hz: 32_000, label: 'High', note: 'clear speech with headroom' },
  { hz: 24_000, label: 'Good', note: 'a sensible middle' },
  { hz: 16_000, label: 'Voice', note: 'speech, a third of the disk' },
  { hz: 8_000, label: 'Telephone', note: 'intelligible, smallest files' },
] as const;

export function RecordingQuality() {
  const settings = useSettingsStore((state) => state.settings);
  const draft = useSettingsStore((state) => state.draft);
  const edit = useSettingsStore((state) => state.edit);
  const deviceRate = useStatusStore((state) => state.status?.capture.sampleRate ?? 0);

  if (!settings) {
    return <p className="text-muted-foreground text-sm">Loading...</p>;
  }

  const pending = { ...settings, ...draft };
  const value = pending.recordingSampleRate === null ? DEVICE_RATE : String(pending.recordingSampleRate);

  // Offering more than the device produces would only invent detail and cost disk, so the ladder stops
  // at whatever the hardware is actually running at.
  const available = deviceRate > 0 ? RATES.filter((rate) => rate.hz <= deviceRate) : RATES;
  const effectiveHz =
    pending.recordingSampleRate === null
      ? deviceRate
      : Math.min(pending.recordingSampleRate, deviceRate || pending.recordingSampleRate);

  return (
    <div className="space-y-3">
      <Label htmlFor="recording-rate">Recording bit rate</Label>

      <Select
        value={value}
        onValueChange={(next) =>
          edit({ recordingSampleRate: next === DEVICE_RATE ? null : Number(next) })
        }
      >
        <SelectTrigger id="recording-rate" className="w-full">
          <AudioLines className="size-3.5 shrink-0 opacity-70" />
          <SelectValue placeholder="Choose a bit rate" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={DEVICE_RATE}>
            <span className="flex flex-col gap-0.5 py-0.5">
              <span>Match the device</span>
              <span className="text-muted-foreground text-[11px] tabular">
                {deviceRate > 0
                  ? `${pcmBitRateKbps(deviceRate)} kbps at ${(deviceRate / 1000).toFixed(1)} kHz`
                  : 'whatever the microphone offers'}
              </span>
            </span>
          </SelectItem>

          {available.map((rate) => (
            <SelectItem key={rate.hz} value={String(rate.hz)}>
              <span className="flex flex-col gap-0.5 py-0.5">
                <span>
                  {rate.label}, {pcmBitRateKbps(rate.hz)} kbps
                </span>
                <span className="text-muted-foreground text-[11px] tabular">
                  {(rate.hz / 1000).toFixed(0)} kHz &middot; {formatBytes(pcmBytesPerHour(rate.hz))} per
                  hour &middot; {rate.note}
                </span>
              </span>
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <p className="text-muted-foreground text-xs">
        Recordings are uncompressed, so the bit rate is exactly what reaches the disk:{' '}
        {effectiveHz > 0 ? (
          <span className="tabular">
            {pcmBitRateKbps(effectiveHz)} kbps is {formatBytes(pcmBytesPerHour(effectiveHz))} per hour
          </span>
        ) : (
          'the figure appears once capture has started'
        )}
        . A lower rate loses treble first, which speech barely uses. Applies to the next recording
        session, and existing recordings keep their own rate.
      </p>
    </div>
  );
}
