import type {
  Bookmark,
  BookmarkListResponse,
  CreateBookmarkRequest,
  HealthResponse,
  ListBookmarksParams,
  UpdateBookmarkRequest
} from "../../../shared/types";

import { toApiError } from "@/src/lib/errors";
import { normalizeServerUrl } from "./useSettings";

export interface LatchApi {
  health: () => Promise<HealthResponse>;
  listBookmarks: (params?: ListBookmarksParams) => Promise<BookmarkListResponse>;
  createBookmark: (payload: CreateBookmarkRequest) => Promise<Bookmark>;
  updateBookmark: (id: string, payload: UpdateBookmarkRequest) => Promise<Bookmark>;
  deleteBookmark: (id: string) => Promise<Bookmark>;
  recordBookmarkOpen: (id: string) => Promise<Bookmark>;
}

export function createLatchApi(serverUrl: string): LatchApi {
  const baseUrl = normalizeServerUrl(serverUrl);

  async function request<T>(path: string, init?: RequestInit) {
    const response = await fetch(buildUrl(path).toString(), {
      ...init,
      headers: {
        "Content-Type": "application/json",
        ...(init?.headers ?? {})
      }
    });
    const raw = await response.text();
    const parsed = raw ? JSON.parse(raw) : null;

    if (!response.ok) {
      throw toApiError(response, parsed);
    }

    return parsed as T;
  }

  function buildUrl(path: string, params?: Record<string, number | string | undefined>) {
    const url = new URL(path, `${baseUrl}/`);
    for (const [key, value] of Object.entries(params ?? {})) {
      if (value === undefined || value === "") {
        continue;
      }
      url.searchParams.set(key, String(value));
    }
    return url;
  }

  return {
    health: () => request<HealthResponse>("/health"),
    listBookmarks: (params: ListBookmarksParams = {}) =>
      request<BookmarkListResponse>(
        buildUrl("/api/bookmarks", {
          q: params.q,
          tag: params.tag,
          url: params.url,
          since: params.since,
          until: params.until,
          offset: params.offset,
          limit: params.limit ?? 100
        }).toString()
      ),
    createBookmark: (payload: CreateBookmarkRequest) =>
      request<Bookmark>("/api/bookmarks", {
        method: "POST",
        body: JSON.stringify(payload)
      }),
    updateBookmark: (id: string, payload: UpdateBookmarkRequest) =>
      request<Bookmark>(`/api/bookmarks/${id}`, {
        method: "PATCH",
        body: JSON.stringify(payload)
      }),
    deleteBookmark: (id: string) =>
      request<Bookmark>(`/api/bookmarks/${id}`, {
        method: "DELETE"
      }),
    recordBookmarkOpen: (id: string) =>
      request<Bookmark>(`/api/bookmarks/${id}/open`, {
        method: "POST"
      })
  };
}
