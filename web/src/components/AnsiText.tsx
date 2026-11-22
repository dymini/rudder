import { useMemo, type CSSProperties } from "react";
import { parseAnsi, readableColor, type Rgb } from "../ansi";

export function AnsiText({ children, console: onConsole = false }: {
  children: string;
  console?: boolean;
}) {
  const runs = useMemo(() => parseAnsi(children), [children]);
  const light: Rgb = [239, 237, 232];
  const dark: Rgb = [35, 33, 30];
  return <span className={onConsole ? "ansi-console" : undefined}>{runs.map((run, index) => {
    const { foreground, background, bold, dim, italic, underline } = run.style;
    const style = {
      "--ansi-light": foreground || background ? readableColor(foreground ?? [28, 25, 23], background ?? light) : undefined,
      "--ansi-dark": foreground || background ? readableColor(foreground ?? [216, 210, 200], background ?? dark) : undefined,
      backgroundColor: background ? `rgb(${background.join(", ")})` : undefined,
      fontWeight: bold ? 700 : undefined,
      opacity: dim ? 0.8 : undefined,
      fontStyle: italic ? "italic" : undefined,
      textDecoration: underline ? "underline" : undefined,
    } as CSSProperties;
    return <span key={index} className="ansi-text" style={style}>{run.text}</span>;
  })}</span>;
}
