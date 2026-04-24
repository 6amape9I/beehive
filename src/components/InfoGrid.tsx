interface InfoGridItem {
  label: string;
  value: string | number | null | undefined;
}

export function InfoGrid({ items }: { items: InfoGridItem[] }) {
  return (
    <dl className="info-grid">
      {items.map((item) => (
        <div className="info-item" key={item.label}>
          <dt>{item.label}</dt>
          <dd>{item.value ?? "Not available"}</dd>
        </div>
      ))}
    </dl>
  );
}
