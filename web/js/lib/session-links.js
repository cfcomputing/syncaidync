import { api } from "./api.js";

export async function sessionLinks(id) {
  try {
    const links = await api.get(`/sessions/${encodeURIComponent(id)}/links`);
    return [
      { label: "Open repo", url: links.repo_url, iconName: "github" },
      { label: "Open branch", url: links.branch_url, iconName: "branch" },
      { label: `Open pull request #${links.pull?.number}`, url: links.pull?.url, iconName: "pull-request" },
    ].filter(({ url }) => typeof url === "string" && url.startsWith("https://github.com/"));
  } catch {
    return [];
  }
}
