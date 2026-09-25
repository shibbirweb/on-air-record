/**
 * Switch two factor sign in on or off for your own account, and replace recovery codes.
 *
 * Switching on is three steps: scan the QR code, type the code the app then shows, and save the recovery
 * codes. Nothing changes for signing in until the second step succeeds, so closing the dialog half way
 * leaves the account exactly as it was.
 */

import { Check, Copy, Download, Loader2, ShieldCheck } from 'lucide-react';
import { useEffect, useId, useState, type FormEvent, type ReactNode } from 'react';

import { ApiError } from '@/api/client';
import type { TwoFactorSetup } from '@/api/types';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { capitalise } from '@/lib/credentials';
import { recoveryCodesFile, svgDataUri } from '@/lib/twoFactor';
import { useAuthStore } from '@/store/useAuthStore';
import { useTwoFactorStore } from '@/store/useTwoFactorStore';

type TwoFactorDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
};

type Step =
  | { kind: 'overview' }
  | { kind: 'scan'; setup: TwoFactorSetup }
  | { kind: 'codes'; codes: string[] }
  | { kind: 'confirm'; action: 'disable' | 'regenerate' };

const describe = (cause: unknown) =>
  cause instanceof ApiError ? capitalise(cause.message) : 'Something went wrong. Try again.';

export function TwoFactorDialog({ open, onOpenChange }: TwoFactorDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        {/* The dialog unmounts its content when it closes, so the steps start again from the overview
            every time it opens, without resetting anything by hand. */}
        <TwoFactorSteps onClose={() => onOpenChange(false)} />
      </DialogContent>
    </Dialog>
  );
}

function TwoFactorSteps({ onClose }: { onClose: () => void }) {
  const status = useTwoFactorStore((state) => state.status);
  const refresh = useTwoFactorStore((state) => state.refresh);
  const beginSetup = useTwoFactorStore((state) => state.beginSetup);
  const [step, setStep] = useState<Step>({ kind: 'overview' });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      setStep({ kind: 'scan', setup: await beginSetup() });
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DialogHeader>
        <DialogTitle>Two factor sign in</DialogTitle>
        <DialogDescription>
          A code from an authenticator app on your phone, asked for after your password. Someone who
          learns your password still cannot sign in without your phone.
        </DialogDescription>
      </DialogHeader>

      {step.kind === 'overview' &&
        (status === null ? (
          <Loader2 className="text-muted-foreground mx-auto size-5 animate-spin" aria-label="Loading" />
        ) : status.enabled ? (
          <div className="space-y-4">
            <p className="flex items-center gap-2 text-sm">
              <ShieldCheck className="text-primary size-5" />
              On. {status.recoveryCodesLeft} of 10 recovery codes left.
            </p>
            {status.recoveryCodesLeft <= 3 && (
              <p className="text-sm">Few recovery codes left. Make new ones before you run out.</p>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setStep({ kind: 'confirm', action: 'regenerate' })}>
                New recovery codes
              </Button>
              <Button variant="destructive" onClick={() => setStep({ kind: 'confirm', action: 'disable' })}>
                Turn off
              </Button>
            </DialogFooter>
          </div>
        ) : (
          <div className="space-y-4">
            <p className="text-sm">
              You need an authenticator app, such as Google Authenticator, Microsoft Authenticator,
              Authy or 1Password. It takes about a minute.
            </p>
            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}
            <DialogFooter>
              <Button disabled={busy} onClick={() => void start()}>
                {busy && <Loader2 className="animate-spin" />}
                Set up
              </Button>
            </DialogFooter>
          </div>
        ))}

      {step.kind === 'scan' && (
        <ScanStep setup={step.setup} onEnabled={(codes) => setStep({ kind: 'codes', codes })} />
      )}

      {step.kind === 'codes' && <RecoveryCodes codes={step.codes} onDone={onClose} />}

      {step.kind === 'confirm' && (
        <ConfirmWithPassword
          action={step.action}
          onCancel={() => setStep({ kind: 'overview' })}
          onRegenerated={(codes) => setStep({ kind: 'codes', codes })}
          onDisabled={() => setStep({ kind: 'overview' })}
        />
      )}
    </>
  );
}

function ScanStep({ setup, onEnabled }: { setup: TwoFactorSetup; onEnabled: (codes: string[]) => void }) {
  const id = useId();
  const enable = useTwoFactorStore((state) => state.enable);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      onEnabled(await enable(code.trim()));
    } catch (cause) {
      setCode('');
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
      <Numbered n={1}>Scan this with your authenticator app.</Numbered>
      <div className="flex justify-center">
        {/* White behind the code whatever the theme, because some phone cameras will not read a dark
            code on a dark background. */}
        <img
          src={svgDataUri(setup.qrSvg)}
          alt="QR code for your authenticator app"
          className="size-48 rounded-md bg-white p-2"
        />
      </div>
      <details className="text-sm">
        <summary className="text-muted-foreground cursor-pointer">Cannot scan it? Type this key instead</summary>
        <div className="mt-2 flex items-center gap-2">
          <code className="bg-muted flex-1 rounded px-2 py-1.5 text-xs tracking-wide break-all">
            {setup.secretKey}
          </code>
          <CopyButton text={setup.secretKey.replaceAll(' ', '')} label="Copy key" />
        </div>
      </details>

      <Numbered n={2}>Type the 6 digit code the app now shows for On Air Record.</Numbered>
      <div className="space-y-2">
        <Label htmlFor={`${id}-code`} className="sr-only">
          Code from the app
        </Label>
        <Input
          id={`${id}-code`}
          autoComplete="one-time-code"
          inputMode="numeric"
          autoFocus
          placeholder="123456"
          className="tabular text-center text-lg tracking-widest"
          value={code}
          onChange={(event) => setCode(event.target.value)}
        />
      </div>

      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}

      <DialogFooter>
        <Button type="submit" disabled={busy || code.trim() === ''}>
          {busy && <Loader2 className="animate-spin" />}
          Turn on
        </Button>
      </DialogFooter>
    </form>
  );
}

function RecoveryCodes({ codes, onDone }: { codes: string[]; onDone: () => void }) {
  const email = useAuthStore((state) => state.user?.email ?? 'your account');

  const download = () => {
    const blob = new Blob([recoveryCodesFile(codes, email, new Date())], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'on-air-record-recovery-codes.txt';
    link.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="space-y-4">
      <p className="flex items-center gap-2 text-sm font-medium">
        <ShieldCheck className="text-primary size-5" />
        Save your recovery codes
      </p>
      <p className="text-muted-foreground text-sm">
        Each one signs you in once if you lose your phone. They are shown only now, so keep them somewhere
        safe and separate from your phone.
      </p>
      <ul className="bg-muted grid grid-cols-2 gap-x-6 gap-y-1 rounded-md px-4 py-3 font-mono text-sm">
        {codes.map((code) => (
          <li key={code}>{code}</li>
        ))}
      </ul>
      <DialogFooter>
        <CopyButton text={codes.join('\n')} label="Copy" />
        <Button variant="outline" onClick={download}>
          <Download />
          Download
        </Button>
        <Button onClick={onDone}>I have saved them</Button>
      </DialogFooter>
    </div>
  );
}

type ConfirmProps = {
  action: 'disable' | 'regenerate';
  onCancel: () => void;
  onRegenerated: (codes: string[]) => void;
  onDisabled: () => void;
};

function ConfirmWithPassword({ action, onCancel, onRegenerated, onDisabled }: ConfirmProps) {
  const id = useId();
  const disable = useTwoFactorStore((state) => state.disable);
  const regenerate = useTwoFactorStore((state) => state.regenerate);
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (action === 'disable') {
        await disable(password);
        onDisabled();
      } else {
        onRegenerated(await regenerate(password));
      }
    } catch (cause) {
      setError(describe(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="space-y-4" onSubmit={(event) => void submit(event)} noValidate>
      <p className="text-sm">
        {action === 'disable'
          ? 'Signing in will need only your password again. Your recovery codes stop working.'
          : 'Your current recovery codes stop working and ten new ones replace them.'}
      </p>
      <div className="space-y-2">
        <Label htmlFor={`${id}-password`}>Your password</Label>
        <Input
          id={`${id}-password`}
          type="password"
          autoComplete="current-password"
          autoFocus
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
      </div>
      {error && (
        <p role="alert" className="text-destructive text-sm">
          {error}
        </p>
      )}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onCancel}>
          Back
        </Button>
        <Button
          type="submit"
          variant={action === 'disable' ? 'destructive' : 'default'}
          disabled={busy || password === ''}
        >
          {busy && <Loader2 className="animate-spin" />}
          {action === 'disable' ? 'Turn off' : 'Make new codes'}
        </Button>
      </DialogFooter>
    </form>
  );
}

function Numbered({ n, children }: { n: number; children: ReactNode }) {
  return (
    <p className="flex items-start gap-2 text-sm">
      <span className="bg-primary text-primary-foreground grid size-5 shrink-0 place-items-center rounded-full text-xs">
        {n}
      </span>
      {children}
    </p>
  );
}

/**
 * Copies to the clipboard where the browser allows it. Browsers only offer the clipboard on HTTPS or
 * localhost, so on a plain `http://192.168...` address the button is not shown and Download remains.
 */
function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);
  if (typeof navigator === 'undefined' || !navigator.clipboard) {
    return null;
  }
  return (
    <Button
      type="button"
      variant="outline"
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        });
      }}
    >
      {copied ? <Check /> : <Copy />}
      {copied ? 'Copied' : label}
    </Button>
  );
}
