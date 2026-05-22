type Props = {
  on: boolean;
  onChange: () => void;
  disabled?: boolean;
  "aria-label"?: string;
};

export default function Toggle({ on, onChange, disabled, ...rest }: Props) {
  return (
    <button
      type="button"
      className="toggle"
      data-on={on}
      disabled={disabled}
      onClick={onChange}
      aria-pressed={on}
      aria-label={rest["aria-label"]}
    >
      <span className="toggle__track" />
      <span className="toggle__dot" />
    </button>
  );
}
