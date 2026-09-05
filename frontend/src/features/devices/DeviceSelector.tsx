/** Picks the microphone the recorder captures from. */

import { AlertTriangle, Mic, RefreshCw } from 'lucide-react';
import { useEffect } from 'react';

import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useDeviceStore } from '@/store/useDeviceStore';
import { useStatusStore } from '@/store/useStatusStore';

/** Sentinel for "no explicit choice", since a Radix select item cannot carry an empty value. */
const SYSTEM_DEFAULT = '__system_default__';

export function DeviceSelector() {
  const devices = useDeviceStore((state) => state.devices);
  const loading = useDeviceStore((state) => state.loading);
  const selecting = useDeviceStore((state) => state.selecting);
  const error = useDeviceStore((state) => state.error);
  const refresh = useDeviceStore((state) => state.refresh);
  const select = useDeviceStore((state) => state.select);

  const activeDeviceName = useStatusStore((state) => state.status?.capture.deviceName ?? null);
  const capturing = useStatusStore((state) => state.status?.capture.state === 'recording');

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const selected = devices.find((device) => device.isSelected);
  const value = selected?.id ?? SYSTEM_DEFAULT;
  const unavailable = selected && !selected.available;

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label htmlFor="input-device">Input source</Label>
        <Button
          size="icon-sm"
          variant="ghost"
          onClick={() => void refresh()}
          disabled={loading}
          aria-label="Rescan devices"
        >
          <RefreshCw className={loading ? 'animate-spin' : undefined} />
        </Button>
      </div>

      <Select
        value={value}
        disabled={selecting}
        onValueChange={(next) => void select(next === SYSTEM_DEFAULT ? null : next)}
      >
        <SelectTrigger id="input-device" className="w-full">
          <SelectValue placeholder="Choose a microphone" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={SYSTEM_DEFAULT}>
            <Mic />
            System default
          </SelectItem>
          {devices.map((device) => (
            <SelectItem key={device.id} value={device.id} disabled={!device.available}>
              <Mic />
              {device.name}
              {device.isDefault && ' (default)'}
              {!device.available && ' (unplugged)'}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {unavailable && (
        <p className="text-destructive flex items-start gap-1.5 text-xs">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          The configured device is not connected. Capture falls back to the system default until it is
          plugged back in.
        </p>
      )}

      {error && <p className="text-destructive text-xs">{error}</p>}

      {activeDeviceName && (
        <div className="text-muted-foreground flex items-center gap-2 text-xs">
          <span>Capturing from</span>
          <Badge variant={capturing ? 'live' : 'secondary'} className="font-normal">
            {activeDeviceName}
          </Badge>
        </div>
      )}
    </div>
  );
}
