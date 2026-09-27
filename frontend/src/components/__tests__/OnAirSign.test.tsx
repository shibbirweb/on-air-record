// @vitest-environment jsdom

/**
 * The on air light. It always reads ON AIR; what changes is whether it is lit, which is only styling, so
 * that is what these check. When it lights is decided by the app shell and tested there.
 */

import '@/test/dom';

import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { OnAirSign } from '../OnAirSign';

const LIT = 'bg-[var(--live)]';

describe('the on air sign', () => {
  it('reads On air', () => {
    render(<OnAirSign live={false} />);
    expect(screen.getByText('On air')).toBeInTheDocument();
  });

  it('is lit red and pulses when live', () => {
    render(<OnAirSign live />);
    const sign = screen.getByText('On air');
    expect(sign).toHaveClass(LIT);
    expect(sign.querySelector('.animate-pulse')).not.toBeNull();
  });

  it('is dim and still when not live', () => {
    render(<OnAirSign live={false} />);
    const sign = screen.getByText('On air');
    expect(sign).not.toHaveClass(LIT);
    expect(sign).toHaveClass('text-muted-foreground');
    expect(sign.querySelector('.animate-pulse')).toBeNull();
  });

  it('takes extra classes from its caller', () => {
    render(<OnAirSign live={false} className="ml-4" />);
    expect(screen.getByText('On air')).toHaveClass('ml-4');
  });

  it('tells a screen reader whether it is on air, not only by colour', () => {
    const { rerender } = render(<OnAirSign live />);
    expect(screen.getByRole('status', { name: 'On air' })).toBeInTheDocument();
    rerender(<OnAirSign live={false} />);
    expect(screen.getByRole('status', { name: 'Off air' })).toBeInTheDocument();
  });
});
