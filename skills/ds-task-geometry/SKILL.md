---
name: ds-task-geometry
description: Read or clear an existing Project Work task's geometry and object links through ds. Geometry authoring from DS Grid structures or alignments is unavailable in Core.
metadata:
  ds-chapters: project
  ds-mcp-profile: project-task-operations
---

# Review a task's existing geometry

Use the `ds` skill first. Keep the task ID, project, lane, and principal explicit.
Discover `pm.task.geometry.read` and read the task's current geometry and object
links. The plan and map's task layer use that stored value; reading it does not
resolve or alter an engineering model.

To remove existing geometry, discover `pm.task.geometry.clear`, review the
selected task, and use its declared confirmation only when that removal is
authorized. Verify the resulting geometry and remaining links through the read
command. Ordinary task creation and checklist operations remain available.

Core has no task geometry authoring command or `--geometry-from`, `--package`,
or `--buffer-m` inputs. Do not derive an area from a comment, hand-edit a model,
or substitute a different command for the removed resolver.

If a request refuses because its project or signed-in identity is missing,
follow its declared remedy without changing the task's intended authority.

Stops at: geometry authoring requires an admitted engineering owner; the operator
supplies that workflow when the installed Core surface does not expose it.
