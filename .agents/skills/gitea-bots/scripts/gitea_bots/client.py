"""Re-export the Gitea HTTP client from the vendored ``issue_labels`` package.

The implementation moved so the standalone label CLI and the bot share one
client; this module keeps the established ``gitea_bots.client`` import path.
"""

from issue_labels.gitea import GiteaClient

__all__ = ["GiteaClient"]
