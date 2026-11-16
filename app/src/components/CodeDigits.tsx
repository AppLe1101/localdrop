interface Props {
  code: string;
  className?: string;
}

export default function CodeDigits({ code, className = "" }: Props) {
  return (
    <span className={`code-digits ${className}`.trim()} aria-label={code}>
      {Array.from(code).map((digit, index) => (
        <span
          className={"code-digit" + (digit === " " ? " code-digit-space" : "")}
          key={`${index}-${digit}`}
          aria-hidden="true"
          style={{ animationDelay: `${index * 45}ms` }}
        >
          {digit === " " ? "\u00a0" : digit}
        </span>
      ))}
    </span>
  );
}
