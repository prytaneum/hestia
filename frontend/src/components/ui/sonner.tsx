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
      containerAriaLabel={m['app.notifications.label']()}
      icons={{
        success: <CheckCircleIcon className="size-4" />,
        info: <InfoIcon className="size-4" />,
        warning: <WarningIcon className="size-4" />,
        error: <XCircleIcon className="size-4" />,
        loading: <SpinnerIcon className="size-4 animate-spin" />,
      }}
      style={
        {
          '--normal-bg': 'var(--popover)',
          '--normal-text': 'var(--popover-foreground)',
          '--normal-border': 'var(--border)',
          '--border-radius': '0px',
          '--toast-close-button-start': 'unset',
          '--toast-close-button-end': '4px',
          '--toast-close-button-transform': 'translateY(4px)',
        } as React.CSSProperties
      }
      toastOptions={{
        style: { fontSize: 'var(--text-xs)' },
        closeButtonAriaLabel: m['app.toast.close'](),
        classNames: {
          toast: 'pr-8',
          title: 'font-medium',
          description: '!text-muted-foreground',
          actionButton: '!rounded-none',
          closeButton:
            '!border-0 !bg-transparent text-muted-foreground hover:text-foreground',
        },
      }}
      {...props}
    />
  );
};

export { Toaster };
