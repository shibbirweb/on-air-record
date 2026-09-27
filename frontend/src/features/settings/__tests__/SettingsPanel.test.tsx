// @vitest-environment jsdom

/**
 * The Audio card: input gain, segment length, record on start up and the start up delay. Each control shows
 * the pending value over the stored one and stages its change in the settings draft to wait for Save. The
 * real settings store is used, so the draft rule (a value put back to what is stored stops counting) is
 * part of what is tested. The sliders are driven from the keyboard, the one input jsdom can give them
 * without a layout.
 */

import '@/test/dom';

import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';

import { useSettingsStore } from '@/store/useSettingsStore';

import { SettingsPanel } from '../SettingsPanel';
import { STORED } from './fixtures';

/** The slider a visible label points at, found the way the label names it. */
function sliderRoot(label: string): HTMLElement {
  const target = screen.getByText(label).getAttribute('for');
  const root = target ? document.getElementById(target) : null;
  if (!root) {
    throw new Error(`no control for the label ${label}`);
  }
  return root;
}

const slider = (label: string) => within(sliderRoot(label)).getByRole('slider');
const autoStartSwitch = () => screen.getByRole('switch', { name: 'Record on start up' });

describe('the sliders', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, draft: {}, error: null });
  });

  it('are named for a screen reader by what they set', () => {
    render(<SettingsPanel />);
    for (const name of ['Input gain', 'Segment length', 'Start up delay']) {
      expect(screen.getByRole('slider', { name })).toBeInTheDocument();
    }
  });
});

describe('the audio settings card', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
  });

  it('says it is loading until the settings arrive', () => {
    useSettingsStore.setState({ settings: null });
    render(<SettingsPanel />);
    expect(screen.getByText('Loading settings...')).toBeInTheDocument();
    expect(screen.queryByRole('slider')).not.toBeInTheDocument();
  });

  it('shows the stored values', () => {
    render(<SettingsPanel />);
    expect(screen.getByText('1.00x')).toBeInTheDocument();
    expect(screen.getByText('10 s')).toBeInTheDocument();
    expect(screen.getByText('None')).toBeInTheDocument();
    expect(slider('Input gain')).toHaveAttribute('aria-valuenow', '1');
    expect(slider('Segment length')).toHaveAttribute('aria-valuenow', '10');
    expect(autoStartSwitch()).toBeChecked();
  });

  it('shows pending edits over the stored values', () => {
    useSettingsStore.setState({
      draft: { gain: 2.5, segmentSeconds: 30, autoStart: false, autoStartDelaySeconds: 20 },
    });
    render(<SettingsPanel />);
    expect(screen.getByText('2.50x')).toBeInTheDocument();
    expect(screen.getByText('30 s')).toBeInTheDocument();
    expect(screen.getByText('20 s')).toBeInTheDocument();
    expect(autoStartSwitch()).not.toBeChecked();
  });

  it('stages a gain change in steps of 0.05 and shows it at once', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    slider('Input gain').focus();
    await user.keyboard('{ArrowRight}');

    expect(useSettingsStore.getState().draft).toEqual({ gain: 1.05 });
    expect(screen.getByText('1.05x')).toBeInTheDocument();
  });

  it('stops counting the gain as a change once it is put back', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    slider('Input gain').focus();
    await user.keyboard('{ArrowRight}{ArrowLeft}');

    expect(useSettingsStore.getState().draft).toEqual({});
    expect(screen.getByText('1.00x')).toBeInTheDocument();
  });

  it('lets the gain go from silence up to four times', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    slider('Input gain').focus();

    await user.keyboard('{End}');
    expect(useSettingsStore.getState().draft).toEqual({ gain: 4 });
    await user.keyboard('{Home}');
    expect(useSettingsStore.getState().draft).toEqual({ gain: 0 });
  });

  it('stages the segment length in five second steps, between 5 and 60 seconds', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    slider('Segment length').focus();

    await user.keyboard('{ArrowRight}');
    expect(useSettingsStore.getState().draft).toEqual({ segmentSeconds: 15 });
    expect(screen.getByText('15 s')).toBeInTheDocument();

    await user.keyboard('{Home}');
    expect(useSettingsStore.getState().draft).toEqual({ segmentSeconds: 5 });
    await user.keyboard('{End}');
    expect(useSettingsStore.getState().draft).toEqual({ segmentSeconds: 60 });
  });

  it('stages switching record on start up off, and forgets it when switched back on', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);

    await user.click(autoStartSwitch());
    expect(useSettingsStore.getState().draft).toEqual({ autoStart: false });
    expect(autoStartSwitch()).not.toBeChecked();

    await user.click(autoStartSwitch());
    expect(useSettingsStore.getState().draft).toEqual({});
  });

  it('stages a start up delay in five second steps, up to two minutes', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    slider('Start up delay').focus();

    await user.keyboard('{ArrowRight}');
    expect(useSettingsStore.getState().draft).toEqual({ autoStartDelaySeconds: 5 });
    expect(screen.getByText('5 s')).toBeInTheDocument();

    await user.keyboard('{End}');
    expect(useSettingsStore.getState().draft).toEqual({ autoStartDelaySeconds: 120 });
  });

  it('disables the start up delay as soon as record on start up is switched off', async () => {
    const user = userEvent.setup();
    render(<SettingsPanel />);
    expect(sliderRoot('Start up delay')).not.toHaveAttribute('aria-disabled', 'true');

    await user.click(autoStartSwitch());
    expect(sliderRoot('Start up delay')).toHaveAttribute('aria-disabled', 'true');
    expect(slider('Start up delay')).not.toHaveAttribute('tabindex');
  });

  it('disables the start up delay when record on start up is stored as off', () => {
    useSettingsStore.setState({ settings: { ...STORED, autoStart: false, autoStartDelaySeconds: 30 } });
    render(<SettingsPanel />);
    expect(sliderRoot('Start up delay')).toHaveAttribute('aria-disabled', 'true');
    // The stored delay is still shown, so switching back on restores what was there.
    expect(screen.getByText('30 s')).toBeInTheDocument();
  });

  it('says the gain applies on save, not the moment the slider moves', () => {
    render(<SettingsPanel />);
    expect(screen.getByText(/Applied to the live signal on save/)).toBeInTheDocument();
  });
});
