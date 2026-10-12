import { json } from '@codemirror/lang-json';
import { yaml } from '@codemirror/lang-yaml';
import {
  HighlightStyle,
  StreamLanguage,
  syntaxHighlighting,
} from '@codemirror/language';
import { properties } from '@codemirror/legacy-modes/mode/properties';
import { toml } from '@codemirror/legacy-modes/mode/toml';
import type { Extension } from '@codemirror/state';
import { EditorView, keymap } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { basicSetup } from 'codemirror';
import { useEffect, useRef } from 'react';

import type { EditorLanguage } from '@/features/servers/lib';

const LANGUAGES: Record<EditorLanguage, () => Extension> = {
  json: () => json(),
  yaml: () => yaml(),
  properties: () => StreamLanguage.define(properties),
  toml: () => StreamLanguage.define(toml),
  plain: () => [],
};

// Read from the app's own tokens, so the editor follows the theme it sits in.
const theme = EditorView.theme({
  '&': {
    height: '100%',
    fontSize: '12px',
    color: 'var(--foreground)',
    backgroundColor: 'var(--card)',
  },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono, ui-monospace, monospace)' },
  '.cm-content': { caretColor: 'var(--foreground)' },
  '.cm-cursor': { borderLeftColor: 'var(--foreground)' },
  '.cm-gutters': {
    color: 'var(--muted-foreground)',
    backgroundColor: 'var(--card)',
    borderRight: '1px solid var(--border)',
  },
  '.cm-activeLine, .cm-activeLineGutter': {
    backgroundColor: 'color-mix(in oklab, var(--muted) 50%, transparent)',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection':
    {
      backgroundColor:
        'color-mix(in oklab, var(--color-ember) 30%, transparent)',
    },
});

const highlight = HighlightStyle.define([
  { tag: [tags.comment, tags.meta], color: 'var(--muted-foreground)' },
  {
    tag: [tags.string, tags.special(tags.string)],
    color: 'var(--color-amber)',
  },
  {
    tag: [tags.number, tags.bool, tags.null, tags.atom],
    color: 'var(--color-ember)',
  },
  {
    tag: [tags.propertyName, tags.definition(tags.propertyName), tags.keyword],
    color: 'var(--foreground)',
    fontWeight: '500',
  },
  { tag: tags.invalid, color: 'var(--destructive)' },
]);

/**
 * A text editor over CodeMirror. Uncontrolled: `value` seeds the document once,
 * and the parent re-mounts it (by key) to open another file — feeding every
 * keystroke back through props would fight the editor's own state.
 */
export default function CodeEditor({
  value,
  language,
  onChange,
  onSave,
}: {
  value: string;
  language: EditorLanguage;
  onChange: (value: string) => void;
  onSave: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const handlers = useRef({ onChange, onSave });
  handlers.current = { onChange, onSave };
  const initial = useRef(value);

  useEffect(() => {
    if (!host.current) return;
    const view = new EditorView({
      parent: host.current,
      doc: initial.current,
      extensions: [
        keymap.of([
          {
            key: 'Mod-s',
            run: () => {
              handlers.current.onSave();
              return true;
            },
          },
        ]),
        basicSetup,
        LANGUAGES[language](),
        theme,
        syntaxHighlighting(highlight),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            handlers.current.onChange(update.state.doc.toString());
          }
        }),
      ],
    });
    return () => view.destroy();
  }, [language]);

  return <div ref={host} className="h-full min-h-0 overflow-hidden" />;
}
