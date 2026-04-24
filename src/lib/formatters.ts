export function formatDateTime(value: string | null | undefined) {
  if (!value) {
    return "Not available";
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }

  return date.toLocaleString();
}

export function shortChecksum(value: string) {
  if (!value) {
    return "Not available";
  }

  return value.length > 16 ? `${value.slice(0, 12)}...${value.slice(-4)}` : value;
}

export function titleFromStatus(value: string) {
  return value.replaceAll("_", " ");
}
