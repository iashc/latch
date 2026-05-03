import { Action, ActionPanel, Icon, List } from "@raycast/api";
import { useEffect, useState } from "react";

import type { TagSummary } from "../../shared/types";
import { BookmarkListView } from "./components/BookmarkListView";
import { listTags } from "./lib/api";

export default function BrowseTagsCommand() {
  const [tags, setTags] = useState<TagSummary[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function load() {
      setIsLoading(true);
      try {
        const response = await listTags();
        if (!cancelled) {
          setTags(response.data);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    void load();

    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <List isLoading={isLoading} navigationTitle="Tags">
      {tags.map((tag) => (
        <List.Item
          key={tag.name}
          icon={Icon.Tag}
          title={tag.name}
          accessories={[{ text: `${tag.count}` }]}
          actions={
            <ActionPanel>
              <Action.Push
                title="View Bookmarks with This Tag"
                target={
                  <BookmarkListView
                    fixedTag={tag.name}
                    navigationTitle={`Tag: ${tag.name}`}
                  />
                }
              />
            </ActionPanel>
          }
        />
      ))}
    </List>
  );
}
