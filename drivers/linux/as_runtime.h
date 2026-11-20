/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef AS_RUNTIME_H
#define AS_RUNTIME_H
#include "as_protocol.h"
struct task_struct;
struct workqueue_struct;
struct as_message;
struct as_runtime_profile;
struct as_runtime_scope;
/* Register real inode/mm-scoped probes, inactive until Core commits admission. */
struct as_runtime_profile *as_runtime_prepare(const void *body, size_t bytes,
                                             struct workqueue_struct *release_queue);
void as_runtime_adopt(struct as_runtime_profile *profile);
void as_runtime_discard(struct as_runtime_profile *profile);
void as_runtime_clear(struct workqueue_struct *release_queue);
/* Called at the actual source replacement; executable probes remain installed. */
void as_runtime_source_changed(const u8 source[16]);
void as_runtime_retire(void);
void as_runtime_forget_task(struct task_struct *task, bool end_profile);
/* Borrow only the live original worker callback, never a later heap lookup. */
struct as_runtime_scope *as_runtime_scope_get(void);
void as_runtime_scope_put(struct as_runtime_scope *scope);
bool as_runtime_scope_reference(const struct as_runtime_scope *scope,
                                struct as_runtime_key *request);
bool as_runtime_scope_active(const struct as_runtime_scope *scope);
/* A live enclosing API is a candidate scope, never an owning queue request. */
bool as_runtime_scope_enclosing(const struct as_runtime_scope *scope, struct as_runtime_key *request);
void as_runtime_child_created(struct task_struct *parent, struct task_struct *child);
/* Core's original allocator/FIFO; source bodies are filled directly, not copied. */
struct as_message *as_runtime_message_alloc(u32 phase,
    const struct as_runtime_key *request, u32 operation, size_t phase_bytes,
    void **phase_body, struct as_runtime_key *observation);
void as_runtime_message_publish(struct as_message *message, size_t phase_bytes);
#endif
