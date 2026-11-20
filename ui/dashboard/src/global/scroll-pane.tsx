import type { ReactNode } from 'react';

interface ScrollPaneProps {
  as?: 'div' | 'aside';
  id?: string;
  className?: string;
  label?: string;
  labelledBy?: string;
  always?: boolean;
  children: ReactNode;
}

export function ScrollPane({ as: Element = 'div', id, className, label, labelledBy, always, children }: ScrollPaneProps) {
  return <Element id={id} className={className ? 'scroll-pane ' + className : 'scroll-pane'}
    role={Element === 'div' ? 'region' : undefined} aria-label={label} aria-labelledby={labelledBy}
    tabIndex={0} data-scroll-always={always || undefined}>
    {children}
  </Element>;
}
