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
    <List isLoading={isLoading} navigationTitle="标签">
      {tags.map((tag) => (
        <List.Item
          key={tag.name}
          icon={Icon.Tag}
          title={tag.name}
          accessories={[{ text: `${tag.count}` }]}
          actions={
            <ActionPanel>
              <Action.Push
                title="查看该标签下的书签"
                target={
                  <BookmarkListView
                    fixedTag={tag.name}
                    navigationTitle={`标签：${tag.name}`}
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
