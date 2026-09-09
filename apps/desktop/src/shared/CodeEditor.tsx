import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import {
  HighlightStyle,
  bracketMatching,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  drawSelection,
  dropCursor,
  keymap,
} from "@codemirror/view";
import { tags } from "@lezer/highlight";
import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import {
  externalValueTransaction,
  externalValueUpdate,
  languageExtension,
  type CodeLanguage,
} from "./codeEditorSupport";

export type CodeEditorProps = {
  ariaLabel: string;
  autoGrow?: boolean;
  className?: string;
  height?: CSSProperties["height"];
  language: CodeLanguage;
  maxHeight?: CSSProperties["maxHeight"];
  minHeight?: CSSProperties["minHeight"];
  onChange: (value: string) => void;
  readOnly?: boolean;
  value: string;
};

export function CodeEditor({
  ariaLabel,
  autoGrow = false,
  className = "",
  height,
  language,
  maxHeight,
  minHeight,
  onChange,
  readOnly = false,
  value,
}: CodeEditorProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  const languageCompartment = useRef(new Compartment());
  const readOnlyCompartment = useRef(new Compartment());
  const accessibilityCompartment = useRef(new Compartment());
  const [initializationError, setInitializationError] = useState("");
  onChangeRef.current = onChange;

  useEffect(() => {
    if (!parentRef.current) return;
    try {
      const view = new EditorView({
        parent: parentRef.current,
        state: EditorState.create({
          doc: value,
          extensions: [
            history(),
            drawSelection(),
            dropCursor(),
            indentOnInput(),
            bracketMatching(),
            keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
            syntaxHighlighting(vividariumHighlightStyle),
            vividariumEditorTheme,
            languageCompartment.current.of(languageExtension(language)),
            readOnlyCompartment.current.of(readOnlyExtensions(readOnly)),
            accessibilityCompartment.current.of(accessibilityAttributes(ariaLabel, readOnly)),
            EditorView.updateListener.of((update) => {
              if (
                update.docChanged
                && !update.transactions.some((transaction) => transaction.annotation(externalValueUpdate))
              ) {
                onChangeRef.current(update.state.doc.toString());
              }
            }),
          ],
        }),
      });
      viewRef.current = view;
      return () => {
        viewRef.current = null;
        view.destroy();
      };
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      console.error(`CodeEditor initialization failed for ${language}:`, error);
      setInitializationError(`Code editor could not be initialized: ${message}`);
    }
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    const transaction = externalValueTransaction(view.state, value);
    if (transaction) view.dispatch(transaction);
  }, [value]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: languageCompartment.current.reconfigure(languageExtension(language)),
    });
  }, [language]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: readOnlyCompartment.current.reconfigure(readOnlyExtensions(readOnly)),
    });
  }, [readOnly]);

  useEffect(() => {
    viewRef.current?.dispatch({
      effects: accessibilityCompartment.current.reconfigure(accessibilityAttributes(ariaLabel, readOnly)),
    });
  }, [ariaLabel, readOnly]);

  const classes = [
    "code-editor",
    `language-${language}`,
    autoGrow ? "code-editor-auto-grow" : "",
    readOnly ? "code-editor-read-only" : "",
    className,
  ].filter(Boolean).join(" ");

  return (
    <div
      className={classes}
      style={{ height, maxHeight, minHeight }}
    >
      {initializationError
        ? <div className="inline-error code-editor-error" role="alert">{initializationError}</div>
        : <div className="code-editor-mount" ref={parentRef} />}
    </div>
  );
}

function readOnlyExtensions(readOnly: boolean) {
  return [
    EditorState.readOnly.of(readOnly),
    EditorView.editable.of(!readOnly),
    EditorView.editorAttributes.of({ class: readOnly ? "cm-readonly" : "cm-editable" }),
  ];
}

function accessibilityAttributes(ariaLabel: string, readOnly: boolean) {
  return EditorView.contentAttributes.of({
    "aria-label": ariaLabel,
    "aria-readonly": String(readOnly),
  });
}

const vividariumEditorTheme = EditorView.theme({
  "&": {
    width: "100%",
    height: "100%",
    color: "var(--text)",
    backgroundColor: "var(--code)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    overflow: "auto",
    overscrollBehavior: "contain",
    fontFamily: '"SFMono-Regular", Consolas, monospace',
    fontSize: "12px",
    lineHeight: "1.65",
  },
  ".cm-content": {
    minWidth: "max-content",
    padding: "16px",
    caretColor: "var(--text)",
  },
  ".cm-line": { padding: "0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
  ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--code-selection) !important",
  },
  ".cm-content[contenteditable=false]": { caretColor: "transparent" },
});

const vividariumHighlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--code-keyword)" },
  { tag: [tags.string, tags.special(tags.string)], color: "var(--code-string)" },
  { tag: [tags.number, tags.bool, tags.null], color: "var(--code-number)" },
  { tag: [tags.lineComment, tags.blockComment], color: "var(--code-comment)", fontStyle: "italic" },
  { tag: tags.function(tags.variableName), color: "var(--code-function)" },
  { tag: [tags.variableName, tags.propertyName], color: "var(--code-variable)" },
  { tag: [tags.operator, tags.bracket], color: "var(--code-operator)" },
]);
