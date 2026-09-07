interface Props {
  orientation: "vertical" | "horizontal";
  label: string;
  className: string;
  value: number;
  min: number;
  max: number;
  defaultValue: number;
  onChange: (value: number) => void;
  onPointerDown: React.PointerEventHandler<HTMLDivElement>;
}

/** The sidebar grows rightward; the bottom panel grows upward. */
export default function ResizeHandle(props: Props) {
  const change = (value: number) => props.onChange(Math.max(props.min, Math.min(props.max, value)));
  return <div
    className={props.className}
    role="separator"
    tabIndex={0}
    aria-orientation={props.orientation}
    aria-label={props.label}
    aria-valuemin={props.min}
    aria-valuemax={props.max}
    aria-valuenow={props.value}
    aria-valuetext={`${props.value} pixels`}
    title="Drag or use arrow keys to resize; Shift for larger steps. Home/End for limits; Enter to reset."
    onPointerDown={props.onPointerDown}
    onDoubleClick={() => change(props.defaultValue)}
    onKeyDown={(e) => {
      const step = e.shiftKey ? 50 : 10;
      const increase = props.orientation === "vertical" ? "ArrowRight" : "ArrowUp";
      const decrease = props.orientation === "vertical" ? "ArrowLeft" : "ArrowDown";
      let value: number;
      if (e.key === increase) value = props.value + step;
      else if (e.key === decrease) value = props.value - step;
      else if (e.key === "Home") value = props.min;
      else if (e.key === "End") value = props.max;
      else if (e.key === "Enter") value = props.defaultValue;
      else return;
      e.preventDefault();
      e.stopPropagation();
      change(value);
    }}
  />;
}
