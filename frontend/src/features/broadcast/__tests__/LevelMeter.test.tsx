// @vitest-environment jsdom

/**
 * The microphone level meter: the bar follows the average level and a tick marks the peak, both on a
 * decibel scale so ordinary speech moves the bar, both rest at zero while inactive, and a peak close to
 * clipping turns the meter red. The scale itself is meterScale in lib/format, tested there.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { meterScale } from '@/lib/format';

import { LevelMeter } from '../LevelMeter';

/** The track, its filled bar and its peak tick. */
function parts(container: HTMLElement) {
  const track = container.querySelector('.bg-muted') as HTMLElement;
  const [bar, peak] = Array.from(track.children) as HTMLElement[];
  return { bar, peak };
}

describe('the level meter', () => {
  it('fills to the average level on a decibel scale', () => {
    const { container } = render(<LevelMeter rms={0.1} peak={0.3} active />);
    expect(parts(container).bar.style.width).toBe(`${meterScale(0.1) * 100}%`);
    // -20 dBFS is two thirds of the way along a 60 dB scale.
    expect(meterScale(0.1)).toBeCloseTo(2 / 3);
  });

  it('marks the peak with a tick', () => {
    const { container } = render(<LevelMeter rms={0.1} peak={0.3} active />);
    expect(parts(container).peak.style.left).toBe(`${meterScale(0.3) * 100}%`);
  });

  it('keeps the peak tick inside the track at full scale', () => {
    const { container } = render(<LevelMeter rms={0.5} peak={1} active />);
    expect(parts(container).peak.style.left).toBe('99.5%');
  });

  it('shows no peak tick for silence', () => {
    const { container } = render(<LevelMeter rms={0} peak={0} active />);
    expect(parts(container).bar.style.width).toBe('0%');
    expect(parts(container).peak).toBeUndefined();
  });

  it('rests empty with no tick while inactive, whatever the levels say', () => {
    const { container } = render(<LevelMeter rms={0.1} peak={0.3} active={false} />);
    expect(parts(container).bar.style.width).toBe('0%');
    expect(parts(container).peak).toBeUndefined();
  });

  it('draws in the normal colour below the clipping warning', () => {
    const { container } = render(<LevelMeter rms={0.3} peak={0.7} active />);
    expect(parts(container).bar).toHaveClass('bg-primary');
    expect(parts(container).bar).not.toHaveClass('bg-destructive');
    expect(screen.getByText('0 dBFS')).not.toHaveClass('text-destructive');
  });

  it('turns red, bar, tick and scale end, when the peak nears clipping', () => {
    const { container } = render(<LevelMeter rms={0.3} peak={0.8} active />);
    expect(parts(container).bar).toHaveClass('bg-destructive');
    expect(parts(container).peak).toHaveClass('bg-destructive');
    expect(screen.getByText('0 dBFS')).toHaveClass('text-destructive');
  });

  it('does not warn of clipping once stopped, whatever peak it was left with', () => {
    render(<LevelMeter rms={0.3} peak={0.9} active={false} />);
    expect(screen.getByText('0 dBFS')).not.toHaveClass('text-destructive');
  });

  it('labels its scale from -60 to 0 dBFS', () => {
    render(<LevelMeter rms={0} peak={0} active />);
    for (const label of ['-60', '-40', '-20', '0 dBFS']) {
      expect(screen.getByText(label)).toBeInTheDocument();
    }
  });
});
