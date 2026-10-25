import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
  pi.on("session_start", async (event) => {
    if (event.reason === "startup") {
      await pi.exec("agentlink", ["update-if-due"]);
    }
  });
}
