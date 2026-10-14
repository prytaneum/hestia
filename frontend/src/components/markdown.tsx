import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';
import remarkGfm from 'remark-gfm';

import { ExternalLink } from '@/components/external-link';
import { cn } from '@/lib/utils';

/**
 * Renders untrusted Markdown (a content project's description body) with
 * `react-markdown` — remark/rehype under the hood, GFM tables/strikethrough via
 * `remark-gfm`. Modrinth bodies mix raw HTML into their Markdown and CurseForge
 * descriptions are HTML outright, so `rehype-raw` parses it into real elements
 * instead of leaving the tags as literal text;
 * `rehype-sanitize` runs immediately after (never omit it) to strip scripts and
 * event handlers, so a hostile body still cannot inject script. Links open in the
 * system browser rather than navigating the webview.
 */
const proseClass = cn(
  'min-w-0 wrap-anywhere text-sm leading-relaxed text-foreground/90',
  '[&_h1]:mt-4 [&_h1]:mb-2 [&_h1]:font-heading [&_h1]:text-lg [&_h1]:font-semibold',
  '[&_h2]:mt-4 [&_h2]:mb-2 [&_h2]:font-heading [&_h2]:text-base [&_h2]:font-semibold',
  '[&_h3]:mt-3 [&_h3]:mb-1.5 [&_h3]:font-semibold',
  '[&_h4]:mt-3 [&_h4]:mb-1.5 [&_h4]:font-semibold',
  '[&_p]:my-2',
  '[&_ul]:my-2 [&_ul]:list-disc [&_ul]:pl-5',
  '[&_ol]:my-2 [&_ol]:list-decimal [&_ol]:pl-5',
  '[&_li]:my-0.5',
  '[&_code]:rounded [&_code]:bg-muted [&_code]:px-1 [&_code]:py-0.5 [&_code]:font-mono [&_code]:text-[0.85em]',
  '[&_pre]:my-3 [&_pre]:max-w-full [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:bg-muted [&_pre]:p-3',
  '[&_pre_code]:bg-transparent [&_pre_code]:p-0 [&_pre_code]:whitespace-pre',
  '[&_blockquote]:my-2 [&_blockquote]:border-l-2 [&_blockquote]:border-border [&_blockquote]:pl-3 [&_blockquote]:text-muted-foreground',
  '[&_a]:text-ember [&_a]:underline [&_a]:underline-offset-2',
  '[&_img]:my-3 [&_img]:h-auto [&_img]:max-w-full [&_img]:rounded',
  '[&_hr]:my-4 [&_hr]:border-border',
  '[&_table]:w-full [&_table]:text-left',
  '[&_th]:border [&_th]:border-border [&_th]:px-2 [&_th]:py-1',
  '[&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1',
);

export function Markdown({
  children,
  className,
}: {
  children: string;
  className?: string;
}) {
  return (
    <div className={cn(proseClass, className)}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeRaw, rehypeSanitize]}
        components={{
          // A table cannot shrink below its columns' min-content width, so a
          // class on the table alone cannot contain a wide one — it needs a
          // scroller of its own around it.
          table({ children, node: _node, ...rest }) {
            return (
              <div className="my-3 max-w-full overflow-x-auto">
                <table {...rest}>{children}</table>
              </div>
            );
          },
          a({ children, node: _node, ...rest }) {
            return <ExternalLink {...rest}>{children}</ExternalLink>;
          },
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  );
}
