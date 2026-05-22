import { useRef, useState, type ReactNode } from "react";

type Props = {
  text: string;
  children: ReactNode;
  side?: "bottom" | "top" | "left";
  delay?: number;
};

export default function Tooltip({ text, children, side = "bottom", delay = 500 }: Props) {
  const [show, setShow] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  return (
    <span
      className="tt-wrap"
      onMouseEnter={() => {
        timer.current = window.setTimeout(() => setShow(true), delay);
      }}
      onMouseLeave={() => {
        if (timer.current) window.clearTimeout(timer.current);
        setShow(false);
      }}
    >
      {children}
      {show && <span className={`tooltip tooltip--${side}`}>{text}</span>}
    </span>
  );
}
