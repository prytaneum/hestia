import {
  CheckCircleIcon,
  InfoIcon,
  SpinnerIcon,
  WarningIcon,
  XCircleIcon,
} from '@phosphor-icons/react';
import { useSyncExternalStore } from 'react';
import { Toaster as Sonner, type ToasterProps } from 'sonner';
import { m } from '@/paraglide/messages.js';

const root = document.documentElement;

function subscribeToTheme(onChange: () => void) {
  const observer = new MutationObserver(onChange);
  observer.observe(root, { attributes: true, attributeFilter: ['class'] });
  return () => observer.disconnect();
}

const documentTheme = () =>
  root.classList.contains('dark') ? 'dark' : 'light';

const Toaster = ({ ...props }: ToasterProps) => {
  const theme = useSyncExternalStore(subscribeToTheme, documentTheme);
  return (
    <Sonner
      theme={theme}
      className="toaster group"
      position="bottom-right"
      offset={{ bottom: 40, right: 16 }}
      visibleToasts={3}
      closeButton
      containerAriaLabel={m['app.toast.notifications']()}
      icons={{
        success: <CheckCircleIcon className="size-4 text-ember" />,
        info: <InfoIcon className="size-4 text-muted-foreground" />,
        warning: <WarningIcon className="size-4 text-amber" />,
        error: <XCircleIcon className="size-4 text-destructive" />,
        loading: <SpinnerIcon className="size-4 animate-spin" />,
      }}
      style={
        {
          '--normal-bg': 'var(--popover)',
          '--normal-text': 'var(--popover-foreground)',
          '--normal-border':
            'color-mix(in oklab, var(--foreground) 10%, transparent)',
          '--border-radius': '0px',
        } as React.CSSProperties
      }
      toastOptions={{
        style: { fontSize: 'var(--text-xs)' },
        closeButtonAriaLabel: m['app.toast.close'](),
        classNames: {
          toast: 'rounded-none !border-l-2',
          success: '!border-l-ember',
          info: '!border-l-muted-foreground',
          warning: '!border-l-amber',
          error: '!border-l-destructive',
          icon: 'rounded-none',
          actionButton: '!rounded-none',
          cancelButton: '!rounded-none',
          closeButton: '!rounded-none !opacity-100',
        },
      }}
      {...props}
    />
  );
};

export { Toaster };
