<script lang="ts">
  import type { CredentialView } from '$lib/api/schemas';
  import { createCredential, deleteCredential } from '$lib/api/credentials';
  import { ApiError } from '$lib/api/_problem';
  import { goto } from '$app/navigation';
  import { Button } from '$lib/components/ui/button';
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent
  } from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter
  } from '$lib/components/ui/dialog';
  import {
    Table,
    TableHeader,
    TableRow,
    TableHead,
    TableBody,
    TableCell
  } from '$lib/components/ui/table';
  import { Key, Trash2 } from 'lucide-svelte';

  let { data } = $props<{ data: { credentials: CredentialView[] } }>();

  // ── credential list ──────────────────────────────────────────────────────────

  // Additions / deletions are tracked separately from the server snapshot so
  // the page stays reactive if SvelteKit re-runs the load (e.g. focus refresh).
  let added = $state<CredentialView[]>([]);
  let removedIds = $state<Set<string>>(new Set());

  let credentials = $derived([...added, ...data.credentials].filter((c) => !removedIds.has(c.id)));

  // ── add modal ────────────────────────────────────────────────────────────────

  let addOpen = $state(false);
  let addAlias = $state('');
  let addToken = $state('');
  let addSubmitting = $state(false);
  let addError = $state<string | null>(null);

  function openAddModal() {
    addAlias = '';
    addToken = '';
    addError = null;
    addOpen = true;
  }

  function closeAddModal() {
    if (addSubmitting) return;
    addOpen = false;
    addError = null;
  }

  async function addCredential() {
    const alias = addAlias.trim();
    const token = addToken.trim();

    if (!alias) {
      addError = 'Alias is required';
      return;
    }
    if (!token) {
      addError = 'Token is required';
      return;
    }

    addSubmitting = true;
    addError = null;

    try {
      const created = await createCredential(alias, token);
      added = [created, ...added];
      addOpen = false;
    } catch (err) {
      if (err instanceof ApiError && err.isMfaRequired()) {
        await goto('/app/settings/security?mfa=required');
        return;
      }
      addError = err instanceof Error ? err.message : 'Unexpected error';
    } finally {
      addSubmitting = false;
    }
  }

  // ── delete confirmation ───────────────────────────────────────────────────────

  let deleteOpen = $state(false);
  let deletingId = $state<string | null>(null);
  let deleting = $state(false);
  let deleteError = $state<string | null>(null);

  function openDeleteModal(id: string) {
    deletingId = id;
    deleteError = null;
    deleteOpen = true;
  }

  function closeDeleteModal() {
    if (deleting) return;
    deleteOpen = false;
    deleteError = null;
    deletingId = null;
  }

  async function confirmDelete() {
    if (!deletingId) return;

    deleting = true;
    deleteError = null;

    try {
      await deleteCredential(deletingId);
      const idToRemove = deletingId;
      removedIds = new Set([...removedIds, idToRemove]);
      added = added.filter((c) => c.id !== idToRemove);
      deleteOpen = false;
      deletingId = null;
    } catch (err) {
      if (err instanceof ApiError && err.isMfaRequired()) {
        await goto('/app/settings/security?mfa=required');
        return;
      }
      deleteError = err instanceof Error ? err.message : 'Unexpected error';
    } finally {
      deleting = false;
    }
  }
</script>

<svelte:head>
  <title>Integrations — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold mb-2">Integrations</h1>
<p class="text-sm text-muted-foreground mb-6">Manage your Hetzner Cloud API tokens.</p>

<Card>
  <CardHeader class="flex flex-row items-center justify-between">
    <div>
      <CardTitle>Hetzner Cloud</CardTitle>
      <CardDescription>
        API tokens used to provision and manage clusters on your Hetzner account.
      </CardDescription>
    </div>
    <Button variant="outline" onclick={openAddModal} data-testid="add-token-button">
      Add token
    </Button>
  </CardHeader>

  <CardContent>
    {#if credentials.length === 0}
      <div class="flex flex-col items-center justify-center py-10 text-center">
        <Key class="h-8 w-8 text-muted-foreground mb-3" />
        <p class="text-sm font-medium mb-1">No tokens yet.</p>
        <p class="text-sm text-muted-foreground">
          Add your first Hetzner token to start provisioning clusters.
        </p>
      </div>
    {:else}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Alias</TableHead>
            <TableHead>Added</TableHead>
            <TableHead class="w-25">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {#each credentials as c (c.id)}
            <TableRow data-testid={`credential-row-${c.id}`}>
              <TableCell class="font-medium text-sm">{c.alias}</TableCell>
              <TableCell class="text-sm text-muted-foreground">
                {new Date(c.created_at).toLocaleDateString()}
              </TableCell>
              <TableCell>
                <Button
                  variant="outline"
                  size="sm"
                  class="text-destructive hover:text-destructive border-destructive/40 hover:border-destructive hover:bg-destructive/5"
                  onclick={() => openDeleteModal(c.id)}
                  data-testid={`remove-${c.id}`}
                >
                  <Trash2 class="mr-1.5 h-3.5 w-3.5" />
                  Remove
                </Button>
              </TableCell>
            </TableRow>
          {/each}
        </TableBody>
      </Table>
    {/if}
  </CardContent>
</Card>

<!-- Add token dialog -->
<Dialog bind:open={addOpen}>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Add Hetzner token</DialogTitle>
      <DialogDescription>
        Your token is encrypted at rest and never returned after saving.
      </DialogDescription>
    </DialogHeader>

    <div class="space-y-4 py-2">
      <div class="space-y-1.5">
        <Label for="add-alias">Alias</Label>
        <Input
          id="add-alias"
          type="text"
          bind:value={addAlias}
          placeholder="e.g. staging"
          data-testid="add-alias-input"
        />
      </div>

      <div class="space-y-1.5">
        <Label for="add-token">Token</Label>
        <Input
          id="add-token"
          type="password"
          bind:value={addToken}
          placeholder="hv1_…"
          autocomplete="off"
          spellcheck={false}
          data-testid="add-token-input"
        />
      </div>

      {#if addError}
        <div
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          role="alert"
        >
          {addError}
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="outline" onclick={closeAddModal} disabled={addSubmitting}>Cancel</Button>
      <Button onclick={addCredential} disabled={addSubmitting} data-testid="add-token-confirm">
        {addSubmitting ? 'Saving…' : 'Save token'}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

<!-- Delete confirmation dialog -->
<Dialog bind:open={deleteOpen}>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Remove token</DialogTitle>
      <DialogDescription>
        This will prevent any clusters that use this token from scaling or reprovisioning.
      </DialogDescription>
    </DialogHeader>

    {#if deleteError}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
        role="alert"
      >
        {deleteError}
      </div>
    {/if}

    <DialogFooter>
      <Button variant="outline" onclick={closeDeleteModal} disabled={deleting}>Cancel</Button>
      <Button
        variant="destructive"
        onclick={confirmDelete}
        disabled={deleting}
        data-testid="delete-confirm"
      >
        {deleting ? 'Removing…' : 'Remove'}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
