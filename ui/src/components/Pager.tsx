interface PagerProps {
  page: number;
  pageSize: number;
  total: number;
  onPage: (page: number) => void;
}

export function Pager({ page, pageSize, total, onPage }: PagerProps) {
  const pages = Math.max(1, Math.ceil(total / pageSize));
  if (pages <= 1) return null;
  return (
    <nav class="pager" aria-label="pages">
      <button disabled={page === 0} onClick={() => onPage(page - 1)}>
        ‹
      </button>
      <span>
        {page + 1} / {pages}
      </span>
      <button disabled={page >= pages - 1} onClick={() => onPage(page + 1)}>
        ›
      </button>
    </nav>
  );
}
