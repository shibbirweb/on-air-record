/**
 * Month grid, wrapping `react-day-picker`.
 *
 * The library owns the hard parts: keyboard navigation, ARIA roles, locale aware week starts, and month
 * arithmetic. Everything here is styling, expressed as Tailwind classes against the library's class name
 * slots rather than by importing its stylesheet, so the calendar inherits the app's theme tokens and
 * follows the light and dark switch for free.
 */

import { ChevronLeft, ChevronRight } from 'lucide-react';
import { DayPicker as DayPickerPrimitive } from 'react-day-picker';
import type * as React from 'react';

import { buttonVariants } from '@/components/ui/button';
import { cn } from '@/lib/utils';

export type CalendarProps = React.ComponentProps<typeof DayPickerPrimitive>;

function Calendar({ className, classNames, showOutsideDays = true, ...props }: CalendarProps) {
  return (
    <DayPickerPrimitive
      showOutsideDays={showOutsideDays}
      className={cn('w-fit', className)}
      classNames={{
        root: 'w-fit',
        months: 'flex flex-col gap-4',
        month: 'flex flex-col gap-3',
        month_caption: 'flex h-8 items-center justify-center px-8',
        caption_label: 'text-sm font-medium',
        nav: 'flex items-center justify-between absolute inset-x-0 top-0 h-8 px-1 pointer-events-none',
        button_previous: cn(
          buttonVariants({ variant: 'ghost', size: 'icon-sm' }),
          'pointer-events-auto disabled:opacity-30',
        ),
        button_next: cn(
          buttonVariants({ variant: 'ghost', size: 'icon-sm' }),
          'pointer-events-auto disabled:opacity-30',
        ),
        month_grid: 'w-full border-collapse',
        weekdays: 'flex',
        weekday: 'text-muted-foreground w-9 text-[11px] font-normal',
        weeks: 'flex flex-col gap-1 pt-1',
        week: 'flex w-full gap-1',
        day: 'size-9 p-0 text-center',
        day_button: cn(
          'size-9 rounded-md p-0 text-sm font-normal transition-colors',
          'hover:bg-accent hover:text-accent-foreground',
          'focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none',
          // A day with no recording is not a destination, so it reads as inert rather than merely faded.
          'disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-25 disabled:hover:bg-transparent',
        ),
        selected: '[&>button]:bg-primary [&>button]:text-primary-foreground [&>button]:hover:bg-primary',
        today: '[&>button]:ring-ring/60 [&>button]:ring-1',
        outside: 'opacity-40',
        disabled: 'opacity-25',
        hidden: 'invisible',
        ...classNames,
      }}
      components={{
        Chevron: ({ orientation, ...chevronProps }) =>
          orientation === 'left' ? (
            <ChevronLeft className="size-4" {...chevronProps} />
          ) : (
            <ChevronRight className="size-4" {...chevronProps} />
          ),
        ...props.components,
      }}
      // The caption is centred over the nav row, so the month needs a positioning context.
      style={{ position: 'relative', ...props.style }}
      {...props}
    />
  );
}

export { Calendar };
