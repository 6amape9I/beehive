interface PaginationControlsProps {
  page: number;
  pageSize: number;
  total: number;
  disabled: boolean;
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}

const pageSizes = [25, 50, 100, 200];

export function PaginationControls({
  page,
  pageSize,
  total,
  disabled,
  onPageChange,
  onPageSizeChange,
}: PaginationControlsProps) {
  const totalPages = Math.max(1, Math.ceil(total / pageSize));
  const start = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const end = Math.min(total, page * pageSize);

  return (
    <div className="pagination-row">
      <span className="muted">
        {start}-{end} of {total}
      </span>
      <div className="button-row">
        <button
          type="button"
          className="button secondary"
          disabled={disabled || page <= 1}
          onClick={() => onPageChange(page - 1)}
        >
          Previous
        </button>
        <span className="pagination-page">
          Page {page} / {totalPages}
        </span>
        <button
          type="button"
          className="button secondary"
          disabled={disabled || page >= totalPages}
          onClick={() => onPageChange(page + 1)}
        >
          Next
        </button>
        <select
          value={pageSize}
          disabled={disabled}
          onChange={(event) => onPageSizeChange(Number(event.target.value))}
        >
          {pageSizes.map((size) => (
            <option key={size} value={size}>
              {size} / page
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}

