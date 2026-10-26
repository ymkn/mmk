export function CardMetadata({ score, favorite, note, series = false }: {
  score: number;
  favorite: boolean;
  note?: string;
  series?: boolean;
}) {
  const hasNote = Boolean(note?.trim());
  if (score === 0 && !favorite && !hasNote) return null;
  const scoreText = score > 0 ? `+${score}` : String(score);
  return <span className="card-meta">
    {score !== 0 && <span title={`${series ? "シリーズ評価" : "評価"} ${scoreText}`}>{scoreText}</span>}
    <span className="card-indicators">
      {favorite && <span role="img" aria-label="お気に入り" title="お気に入り">★</span>}
      {hasNote && <span className="note-indicator" role="img" aria-label="メモあり" title="メモあり">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <path d="M14 3H5v18h14V8Z" /><path d="M14 3v5h5M8 12h8M8 16h6" />
        </svg>
      </span>}
    </span>
  </span>;
}
