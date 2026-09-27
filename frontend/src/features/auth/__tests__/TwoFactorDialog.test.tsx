// @vitest-environment jsdom

/**
 * The two factor dialog: switching it on (scan, type the code, save the recovery codes), replacing the
 * recovery codes, and switching it off, the last two behind the account password. The two factor store's
 * actions are spies, so these test which action each step calls with what, what each failure says, and
 * that the dialog starts again from the overview every time it opens. Copy uses user-event's clipboard
 * stand in; Download is followed as far as the file handed to the browser.
 */

import '@/test/dom';

import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Mock } from 'vitest';

import { ApiError } from '@/api/client';
import type { TwoFactorSetup, TwoFactorStatus, User } from '@/api/types';
import { svgDataUri } from '@/lib/twoFactor';
import { useAuthStore } from '@/store/useAuthStore';
import { useTwoFactorStore } from '@/store/useTwoFactorStore';

import { TwoFactorDialog } from '../TwoFactorDialog';

const SETUP: TwoFactorSetup = {
  secretKey: 'JBSW Y3DP EHPK 3PXP',
  otpauthUri: 'otpauth://totp/On%20Air%20Record:admin@example.com?secret=JBSWY3DPEHPK3PXP',
  qrSvg: '<svg xmlns="http://www.w3.org/2000/svg"><path id="qr-modules" d="M0 0h1v1H0z"/></svg>',
};

const CODES = [
  'aaaa-1111',
  'bbbb-2222',
  'cccc-3333',
  'dddd-4444',
  'eeee-5555',
  'ffff-6666',
  'gggg-7777',
  'hhhh-8888',
  'iiii-9999',
  'jjjj-0000',
];

const ADMIN: User = {
  id: 1,
  email: 'admin@example.com',
  role: 'admin',
  createdAtMs: 0,
  twoFactorEnabled: false,
};

const OFF: TwoFactorStatus = { enabled: false, recoveryCodesLeft: 0 };
const ON: TwoFactorStatus = { enabled: true, recoveryCodesLeft: 7 };

let refresh: Mock<() => Promise<void>>;
let beginSetup: Mock<() => Promise<TwoFactorSetup>>;
let enable: Mock<(code: string) => Promise<string[]>>;
let disable: Mock<(password: string) => Promise<void>>;
let regenerate: Mock<(password: string) => Promise<string[]>>;
let onOpenChange: Mock<(open: boolean) => void>;

/** Holds the open state the way the account page does, so closing really closes. */
function Harness({ initiallyOpen = true }: { initiallyOpen?: boolean }) {
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        Open the dialog
      </button>
      <TwoFactorDialog
        open={open}
        onOpenChange={(next) => {
          onOpenChange(next);
          setOpen(next);
        }}
      />
    </>
  );
}

const dialog = () => screen.getByRole('dialog', { name: 'Two factor sign in' });
const codeField = () => screen.getByLabelText('Code from the app');
const passwordField = () => screen.getByLabelText('Your password');

/** From the overview of an account without two factor, to the recovery codes. */
async function switchOn(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole('button', { name: 'Set up' }));
  await user.type(await screen.findByLabelText('Code from the app'), '123456');
  await user.click(screen.getByRole('button', { name: 'Turn on' }));
  await screen.findByText('Save your recovery codes');
}

describe('the two factor dialog', () => {
  beforeEach(() => {
    refresh = vi.fn(async () => undefined);
    beginSetup = vi.fn(async () => SETUP);
    enable = vi.fn(async () => CODES);
    disable = vi.fn(async () => undefined);
    regenerate = vi.fn(async () => CODES);
    onOpenChange = vi.fn<(open: boolean) => void>();
    useTwoFactorStore.setState({ status: OFF, refresh, beginSetup, enable, disable, regenerate });
    useAuthStore.setState({ mode: 'accounts', user: ADMIN, loaded: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('when closed', () => {
    it('shows nothing and asks nothing', () => {
      render(<Harness initiallyOpen={false} />);
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
      expect(refresh).not.toHaveBeenCalled();
    });
  });

  describe('its overview', () => {
    it('explains two factor sign in and reads the current status on opening', () => {
      render(<Harness />);
      expect(dialog()).toHaveTextContent('Someone who learns your password still cannot sign in');
      expect(refresh).toHaveBeenCalledTimes(1);
    });

    it('shows a loading sign until the status is known', () => {
      useTwoFactorStore.setState({ status: null });
      render(<Harness />);
      expect(within(dialog()).getByLabelText('Loading')).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: 'Set up' })).not.toBeInTheDocument();
    });

    it('names some authenticator apps and offers Set up when it is off', () => {
      render(<Harness />);
      expect(dialog()).toHaveTextContent('Google Authenticator, Microsoft Authenticator');
      expect(screen.getByRole('button', { name: 'Set up' })).toBeEnabled();
      expect(screen.queryByRole('button', { name: 'Turn off' })).not.toBeInTheDocument();
    });

    it('says it is on and how many recovery codes are left, and offers new codes or turning it off', () => {
      useTwoFactorStore.setState({ status: ON });
      render(<Harness />);
      expect(dialog()).toHaveTextContent('On. 7 of 10 recovery codes left.');
      expect(screen.getByRole('button', { name: 'New recovery codes' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Turn off' })).toBeInTheDocument();
      expect(dialog()).not.toHaveTextContent('Few recovery codes left');
    });

    it('warns when three or fewer recovery codes are left', () => {
      useTwoFactorStore.setState({ status: { enabled: true, recoveryCodesLeft: 3 } });
      render(<Harness />);
      expect(dialog()).toHaveTextContent('Few recovery codes left. Make new ones before you run out.');
    });

    it('closes from its close button', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Close' }));
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });

    it('closes with Escape', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.keyboard('{Escape}');
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  describe('switching it on', () => {
    it('shows the QR code as an image, never as markup', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));

      expect(beginSetup).toHaveBeenCalledTimes(1);
      const image = await screen.findByRole('img', { name: 'QR code for your authenticator app' });
      expect(image).toHaveAttribute('src', svgDataUri(SETUP.qrSvg));
      expect(document.getElementById('qr-modules')).toBeNull();
    });

    it('offers the key to type for an app that cannot scan', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));

      expect(await screen.findByText('Cannot scan it? Type this key instead')).toBeInTheDocument();
      expect(screen.getByText(SETUP.secretKey)).toBeInTheDocument();
    });

    it('copies the key without its spaces', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));
      await user.click(await screen.findByRole('button', { name: 'Copy key' }));

      expect(await navigator.clipboard.readText()).toBe('JBSWY3DPEHPK3PXP');
      expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument();
    });

    it('disables Set up while the setup is being fetched', async () => {
      let answer: (setup: TwoFactorSetup) => void = () => undefined;
      beginSetup.mockImplementation(() => new Promise<TwoFactorSetup>((resolve) => (answer = resolve)));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));

      expect(screen.getByRole('button', { name: 'Set up' })).toBeDisabled();
      await act(async () => {
        answer(SETUP);
      });
      expect(screen.getByLabelText('Code from the app')).toBeInTheDocument();
    });

    it('says why when the setup cannot start, and stays on the overview', async () => {
      beginSetup.mockRejectedValue(new ApiError('two factor sign in is already on', 'conflict', 409));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('Two factor sign in is already on');
      expect(screen.getByRole('button', { name: 'Set up' })).toBeEnabled();
    });

    it('keeps Turn on disabled until a code is typed, with the cursor in the code field', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));

      expect(await screen.findByLabelText('Code from the app')).toHaveFocus();
      expect(screen.getByRole('button', { name: 'Turn on' })).toBeDisabled();
      await user.type(codeField(), '   ');
      expect(screen.getByRole('button', { name: 'Turn on' })).toBeDisabled();
      await user.type(codeField(), '1');
      expect(screen.getByRole('button', { name: 'Turn on' })).toBeEnabled();
    });

    it('turns it on with the code typed, without surrounding spaces', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));
      await user.type(await screen.findByLabelText('Code from the app'), ' 654321 ');
      await user.click(screen.getByRole('button', { name: 'Turn on' }));
      expect(enable).toHaveBeenCalledWith('654321');
    });

    it('disables Turn on while the code is being checked', async () => {
      let answer: (codes: string[]) => void = () => undefined;
      enable.mockImplementation(() => new Promise<string[]>((resolve) => (answer = resolve)));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));
      await user.type(await screen.findByLabelText('Code from the app'), '654321');
      await user.click(screen.getByRole('button', { name: 'Turn on' }));

      expect(screen.getByRole('button', { name: 'Turn on' })).toBeDisabled();
      await act(async () => {
        answer(CODES);
      });
      expect(screen.getByText('Save your recovery codes')).toBeInTheDocument();
    });

    it('empties the field and says why when the code is wrong, staying on the scan step', async () => {
      enable.mockRejectedValue(new ApiError('that code is not right', 'bad_request', 400));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));
      await user.type(await screen.findByLabelText('Code from the app'), '000000');
      await user.click(screen.getByRole('button', { name: 'Turn on' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('That code is not right');
      expect(codeField()).toHaveValue('');
      expect(screen.getByRole('img', { name: 'QR code for your authenticator app' })).toBeInTheDocument();
    });

    it('says something went wrong when the failure is not the server speaking', async () => {
      enable.mockRejectedValue(new TypeError('Failed to fetch'));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Set up' }));
      await user.type(await screen.findByLabelText('Code from the app'), '000000');
      await user.click(screen.getByRole('button', { name: 'Turn on' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('Something went wrong. Try again.');
    });
  });

  describe('the recovery codes', () => {
    it('lists all ten, and says they are shown only now', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await switchOn(user);

      const items = within(dialog()).getAllByRole('listitem');
      expect(items.map((item) => item.textContent)).toEqual(CODES);
      expect(dialog()).toHaveTextContent('They are shown only now');
    });

    it('copies all of them, one per line, and says so for a moment', async () => {
      vi.useFakeTimers({ shouldAdvanceTime: true });
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime.bind(vi) });
      render(<Harness />);
      await switchOn(user);
      await user.click(screen.getByRole('button', { name: 'Copy' }));

      expect(await navigator.clipboard.readText()).toBe(CODES.join('\n'));
      expect(screen.getByRole('button', { name: 'Copied' })).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(1_600);
      });
      expect(screen.getByRole('button', { name: 'Copy' })).toBeInTheDocument();
    });

    it('offers no Copy where the browser gives no clipboard, leaving Download', async () => {
      const user = userEvent.setup();
      Object.defineProperty(window.navigator, 'clipboard', { value: undefined, configurable: true });
      render(<Harness />);
      await switchOn(user);

      expect(screen.queryByRole('button', { name: 'Copy' })).not.toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Download' })).toBeInTheDocument();
    });

    it('downloads them as a text file naming the account', async () => {
      const blobs: Blob[] = [];
      const createObjectURL = vi.fn((blob: Blob) => {
        blobs.push(blob);
        return 'blob:recovery-codes';
      });
      const revokeObjectURL = vi.fn<(url: string) => void>();
      Object.defineProperty(URL, 'createObjectURL', { value: createObjectURL, configurable: true });
      Object.defineProperty(URL, 'revokeObjectURL', { value: revokeObjectURL, configurable: true });
      const clicked: HTMLAnchorElement[] = [];
      const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
        this: HTMLAnchorElement,
      ) {
        clicked.push(this);
      });

      const user = userEvent.setup();
      render(<Harness />);
      await switchOn(user);
      await user.click(screen.getByRole('button', { name: 'Download' }));
      click.mockRestore();

      expect(clicked).toHaveLength(1);
      expect(clicked[0].download).toBe('on-air-record-recovery-codes.txt');
      expect(clicked[0].href).toBe('blob:recovery-codes');
      expect(blobs[0].type).toBe('text/plain');
      const text = await blobs[0].text();
      expect(text).toContain('Account: admin@example.com');
      for (const code of CODES) {
        expect(text).toContain(code);
      }
      expect(revokeObjectURL).toHaveBeenCalledWith('blob:recovery-codes');
    });

    it('closes the dialog with I have saved them', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await switchOn(user);
      await user.click(screen.getByRole('button', { name: 'I have saved them' }));

      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    });
  });

  describe('making new recovery codes', () => {
    beforeEach(() => {
      useTwoFactorStore.setState({ status: ON });
    });

    it('asks for the password first, saying the old codes stop working', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'New recovery codes' }));

      expect(dialog()).toHaveTextContent('Your current recovery codes stop working and ten new ones replace them.');
      expect(passwordField()).toHaveAttribute('type', 'password');
      expect(passwordField()).toHaveFocus();
      expect(screen.getByRole('button', { name: 'Make new codes' })).toBeDisabled();
    });

    it('makes them with the password and shows them', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'New recovery codes' }));
      await user.type(passwordField(), 'my password');
      await user.click(screen.getByRole('button', { name: 'Make new codes' }));

      expect(regenerate).toHaveBeenCalledWith('my password');
      expect(disable).not.toHaveBeenCalled();
      expect(await screen.findByText('Save your recovery codes')).toBeInTheDocument();
      expect(within(dialog()).getAllByRole('listitem')).toHaveLength(10);
    });

    it('says why when the password is wrong, and keeps asking', async () => {
      regenerate.mockRejectedValue(new ApiError('the password is not right', 'bad_request', 400));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'New recovery codes' }));
      await user.type(passwordField(), 'wrong');
      await user.click(screen.getByRole('button', { name: 'Make new codes' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('The password is not right');
      expect(passwordField()).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Make new codes' })).toBeEnabled();
    });

    it('goes back to the overview without asking the server', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'New recovery codes' }));
      await user.click(screen.getByRole('button', { name: 'Back' }));

      expect(screen.getByRole('button', { name: 'New recovery codes' })).toBeInTheDocument();
      expect(regenerate).not.toHaveBeenCalled();
    });
  });

  describe('switching it off', () => {
    beforeEach(() => {
      useTwoFactorStore.setState({ status: ON });
    });

    it('asks for the password first, saying signing in needs only the password again', async () => {
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Turn off' }));

      expect(dialog()).toHaveTextContent('Signing in will need only your password again.');
      expect(screen.getByRole('button', { name: 'Turn off' })).toBeDisabled();
    });

    it('turns it off with the password and returns to the overview', async () => {
      disable.mockImplementation(async () => {
        useTwoFactorStore.setState({ status: OFF });
      });
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Turn off' }));
      await user.type(passwordField(), 'my password');
      await user.click(screen.getByRole('button', { name: 'Turn off' }));

      expect(disable).toHaveBeenCalledWith('my password');
      expect(regenerate).not.toHaveBeenCalled();
      expect(await screen.findByRole('button', { name: 'Set up' })).toBeInTheDocument();
    });

    it('disables Turn off while the request is on its way', async () => {
      let finish: () => void = () => undefined;
      disable.mockImplementation(() => new Promise<void>((resolve) => (finish = resolve)));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Turn off' }));
      await user.type(passwordField(), 'my password');
      await user.click(screen.getByRole('button', { name: 'Turn off' }));

      expect(screen.getByRole('button', { name: 'Turn off' })).toBeDisabled();
      await act(async () => {
        finish();
      });
    });

    it('says why when the password is wrong', async () => {
      disable.mockRejectedValue(new ApiError('the password is not right', 'bad_request', 400));
      const user = userEvent.setup();
      render(<Harness />);
      await user.click(screen.getByRole('button', { name: 'Turn off' }));
      await user.type(passwordField(), 'wrong');
      await user.click(screen.getByRole('button', { name: 'Turn off' }));

      expect(await screen.findByRole('alert')).toHaveTextContent('The password is not right');
      expect(passwordField()).toBeInTheDocument();
    });
  });

  it('starts again from the overview every time it opens', async () => {
    useTwoFactorStore.setState({ status: ON });
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole('button', { name: 'Turn off' }));
    expect(passwordField()).toBeInTheDocument();

    await user.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Open the dialog' }));

    expect(screen.queryByLabelText('Your password')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'New recovery codes' })).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledTimes(2);
  });

  it('names the file after nobody in particular when no account is known', async () => {
    useAuthStore.setState({ user: null });
    const blobs: Blob[] = [];
    Object.defineProperty(URL, 'createObjectURL', {
      value: (blob: Blob) => {
        blobs.push(blob);
        return 'blob:recovery-codes';
      },
      configurable: true,
    });
    Object.defineProperty(URL, 'revokeObjectURL', { value: () => undefined, configurable: true });
    const click = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);

    const user = userEvent.setup();
    render(<Harness />);
    await switchOn(user);
    await user.click(screen.getByRole('button', { name: 'Download' }));
    click.mockRestore();

    expect(await blobs[0].text()).toContain('Account: your account');
  });
});
