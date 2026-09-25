export default function SectionHeading({
  title,
  subtitle,
}: {
  title: string
  subtitle?: string
}) {
  return (
    <div className="mx-auto mb-14 max-w-3xl text-center">
      <h2 className="font-inter text-3xl font-bold leading-tight md:text-4xl">
        {title}
      </h2>
      {subtitle && (
        <p className="mt-4 text-base leading-relaxed text-[var(--desc-color)]">
          {subtitle}
        </p>
      )}
    </div>
  )
}
