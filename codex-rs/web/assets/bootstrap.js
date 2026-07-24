const headingNode = document.querySelector("#bootstrap-heading");
const statusNode = document.querySelector("#bootstrap-status");
const token = new URLSearchParams(location.hash.slice(1)).get("bootstrap");
const requestedReturnTo = new URLSearchParams(location.search).get("returnTo") || "";
const returnTo = requestedReturnTo.startsWith("/") && !requestedReturnTo.startsWith("//")
  ? requestedReturnTo
  : "";

function showError(heading, message) {
  headingNode.textContent = heading;
  statusNode.textContent = message;
  statusNode.classList.remove("text-muted-foreground");
  statusNode.classList.add("text-red-400");
}

async function exchangeBootstrapToken() {
  if (!token) {
    showError(
      "Authentication required",
      "This URL is missing its private access token. Open the complete link printed when typeduck-codex-web started; it ends in #bootstrap=…. If the link was lost, restart the daemon to print it again.",
    );
    return;
  }
  history.replaceState({}, "", "/");
  let response;
  try {
    response = await fetch("/auth/exchange", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token }),
    });
  } catch (_error) {
    showError(
      "Authentication unavailable",
      "Codex Web could not reach its authentication endpoint. Check the server or reverse proxy, then reload the complete private link.",
    );
    return;
  }
  if (!response.ok) {
    showError(
      "Private link rejected",
      "This access token is invalid or has been replaced. Restart typeduck-codex-web and open the newly printed private link.",
    );
    return;
  }
  const result = await response.json();
  location.replace(result.basePath + returnTo);
}

void exchangeBootstrapToken();
