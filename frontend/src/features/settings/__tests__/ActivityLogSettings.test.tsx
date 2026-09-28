// @vitest-environment jsdom

/**
 * The Activity log card: how many days entries are kept, and the way to the log itself. Like the rest of
 * the settings page, a change here waits for Save, so the control stages an edit and nothing more. The
 * real settings store holds the draft.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router';
import { beforeEach, describe, expect, it } from 'vitest';

import { useSettingsStore } from '@/store/useSettingsStore';

import { ActivityLogSettings } from '../ActivityLogSettings';
import { STORED } from './fixtures';

const days = () => screen.getByRole('spinbutton', { name: 'Days to keep the activity log' });
const draft = () => useSettingsStore.getState().draft;

function renderCard() {
  const user = userEvent.setup();
  render(
    <MemoryRouter>
      <ActivityLogSettings />
    </MemoryRouter>,
  );
  return user;
}

describe('the activity log card', () => {
  beforeEach(() => {
    useSettingsStore.setState({ settings: STORED, defaults: STORED, draft: {}, error: null });
  });

  it('shows how long entries are kept now', () => {
    renderCard();
    expect(days()).toHaveValue(90);
  });

  it('shows a change waiting for Save rather than the stored value', () => {
    useSettingsStore.setState({ draft: { activityRetentionDays: 30 } });
    renderCard();
    expect(days()).toHaveValue(30);
  });

  it('stages a preset for Save', async () => {
    const user = renderCard();
    await user.click(screen.getByRole('button', { name: '1 year' }));
    expect(draft()).toEqual({ activityRetentionDays: 365 });
    await user.click(screen.getByRole('button', { name: '30 days' }));
    expect(draft()).toEqual({ activityRetentionDays: 30 });
  });

  it('stages a number typed in', async () => {
    const user = renderCard();
    await user.clear(days());
    await user.type(days(), '45');
    expect(draft()).toEqual({ activityRetentionDays: 45 });
  });

  it('says what it accepts and stages nothing outside it', async () => {
    const user = renderCard();
    await user.clear(days());
    await user.type(days(), '0');
    expect(screen.getByText('Between 1 and 3650 days.')).toBeInTheDocument();
    expect(draft().activityRetentionDays).toBeUndefined();
  });

  it('leads to the log', () => {
    renderCard();
    expect(screen.getByRole('link', { name: 'Open the activity log' })).toHaveAttribute('href', '/activity');
  });

  it('waits for the settings to load', () => {
    useSettingsStore.setState({ settings: null });
    renderCard();
    expect(screen.queryByRole('spinbutton')).not.toBeInTheDocument();
  });
});
