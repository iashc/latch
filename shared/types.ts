export interface Bookmark {
  id: string;
  url: string;
  title: string;
  description: string;
  tags: string[];
  open_count: number;
  last_opened: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface BookmarkListResponse {
  object: "list";
  data: Bookmark[];
  offset: number;
  limit: number;
  total: number;
}

export interface TagSummary {
  name: string;
  count: number;
}

export interface TagListResponse {
  object: "list";
  data: TagSummary[];
  total: number;
}

export interface HealthResponse {
  ok: true;
}

export interface ImportResultResponse {
  object: "import_result";
  created: number;
  restored: number;
  skipped: number;
  total: number;
}

export interface ErrorPayload {
  error: {
    code: string;
    message: string;
    details?: Record<string, unknown>;
  };
}

export interface CreateBookmarkRequest {
  url: string;
  title?: string;
  description?: string;
  tags?: string[];
}

export interface UpdateBookmarkRequest {
  url?: string;
  title?: string;
  description?: string;
  tags?: string[];
}

export interface ListBookmarksParams {
  q?: string;
  tag?: string;
  url?: string;
  since?: string;
  until?: string;
  offset?: number;
  limit?: number;
}
