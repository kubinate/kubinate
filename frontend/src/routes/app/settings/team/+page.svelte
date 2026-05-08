<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import {
    ApiError,
    createInvite,
    listInvites,
    listMembers,
    removeMember,
    revokeInvite,
    updateMemberRole
  } from '$lib/api/team';
  import { getMe } from '$lib/api/me';
  import type { InviteView, MembershipRole, MembershipView } from '$lib/api/schemas';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent
  } from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import { Separator } from '$lib/components/ui/separator';
  import {
    Table,
    TableHeader,
    TableRow,
    TableHead,
    TableBody,
    TableCell
  } from '$lib/components/ui/table';

  // `null` => not loaded yet, `[]` => loaded but empty.
  let members = $state<MembershipView[] | null>(null);
  let invites = $state<InviteView[] | null>(null);
  let organizationId = $state<string | null>(null);
  let loadError = $state<string | null>(null);

  // Invite form fields.
  let inviteEmail = $state('');
  let inviteRole = $state<MembershipRole>('developer');
  let inviteFormError = $state<string | null>(null);
  let inviteSubmitting = $state(false);

  // The token returned by the server is shown once and then forgotten.
  let lastIssuedToken = $state<string | null>(null);

  // Tracks per-row "in flight" so a slow PATCH can disable that row's
  // controls without freezing the whole page.
  const busy = new SvelteSet<string>();

  function setBusy(key: string, value: boolean) {
    if (value) busy.add(key);
    else busy.delete(key);
  }

  function describe(err: unknown): string {
    if (err instanceof ApiError) return `${err.title}: ${err.detail}`;
    if (err instanceof Error) return err.message;
    return 'Unknown error';
  }

  async function refresh() {
    if (!organizationId) return;
    try {
      const [m, i] = await Promise.all([listMembers(organizationId), listInvites(organizationId)]);
      members = m;
      invites = i;
    } catch (err) {
      loadError = describe(err);
    }
  }

  onMount(async () => {
    try {
      const me = await getMe();
      organizationId = me.organization_id;
      await refresh();
    } catch (err) {
      loadError = describe(err);
    }
  });

  async function onCreateInvite(event: SubmitEvent) {
    event.preventDefault();
    inviteFormError = null;
    if (!organizationId) return;
    if (!inviteEmail.includes('@')) {
      inviteFormError = 'Email looks invalid';
      return;
    }
    inviteSubmitting = true;
    try {
      const result = await createInvite(organizationId, inviteEmail, inviteRole);
      lastIssuedToken = result.token;
      inviteEmail = '';
      await refresh();
    } catch (err) {
      inviteFormError = describe(err);
    } finally {
      inviteSubmitting = false;
    }
  }

  async function onRoleChange(member: MembershipView, role: MembershipRole) {
    if (!organizationId) return;
    setBusy(`role-${member.user_id}`, true);
    try {
      await updateMemberRole(organizationId, member.user_id, role);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`role-${member.user_id}`, false);
    }
  }

  async function onRemoveMember(member: MembershipView) {
    if (!organizationId) return;
    setBusy(`remove-${member.user_id}`, true);
    try {
      await removeMember(organizationId, member.user_id);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`remove-${member.user_id}`, false);
    }
  }

  async function onRevokeInvite(invite: InviteView) {
    if (!organizationId) return;
    setBusy(`invite-${invite.id}`, true);
    try {
      await revokeInvite(organizationId, invite.id);
      await refresh();
    } catch (err) {
      loadError = describe(err);
    } finally {
      setBusy(`invite-${invite.id}`, false);
    }
  }

  function inviteStatus(invite: InviteView): string {
    if (invite.accepted_at) return 'accepted';
    if (invite.revoked_at) return 'revoked';
    if (new Date(invite.expires_at) < new Date()) return 'expired';
    return 'pending';
  }

  function roleBadgeVariant(role: string): 'default' | 'secondary' | 'outline' {
    if (role === 'owner' || role === 'admin') return 'default';
    if (role === 'developer') return 'secondary';
    return 'outline';
  }
</script>

<svelte:head>
  <title>Team — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold mb-6">Team</h1>

{#if loadError}
  <div class="mb-4 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive" role="alert">
    {loadError}
  </div>
{/if}

<!-- Section 1: Members -->
<Card class="mb-6">
  <CardHeader>
    <CardTitle>Members</CardTitle>
    <CardDescription>Manage the people in your organization.</CardDescription>
  </CardHeader>
  <CardContent>
    {#if members === null}
      <div class="space-y-2">
        {#each [1, 2, 3] as _ (_.toString())}
          <div class="h-10 rounded-md bg-muted animate-pulse"></div>
        {/each}
      </div>
    {:else if members.length === 0}
      <p class="text-sm text-muted-foreground">No members yet.</p>
    {:else}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Member</TableHead>
            <TableHead>Role</TableHead>
            <TableHead>Joined</TableHead>
            <TableHead class="w-[180px]">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {#each members as member (member.user_id)}
            <TableRow data-testid={`member-row-${member.user_id}`}>
              <TableCell>
                <div class="flex flex-col">
                  <span class="font-medium text-sm">{member.display_name}</span>
                  <span class="text-xs text-muted-foreground">{member.email}</span>
                </div>
              </TableCell>
              <TableCell>
                <Badge variant={roleBadgeVariant(member.role)}>
                  {member.role}
                </Badge>
              </TableCell>
              <TableCell class="text-sm text-muted-foreground">
                {new Date(member.joined_at).toLocaleDateString()}
              </TableCell>
              <TableCell>
                <div class="flex items-center gap-2">
                  <select
                    value={member.role}
                    onchange={(e) =>
                      onRoleChange(
                        member,
                        (e.currentTarget as HTMLSelectElement).value as MembershipRole
                      )}
                    disabled={busy.has(`role-${member.user_id}`)}
                    data-testid={`role-select-${member.user_id}`}
                    class="h-8 rounded-md border border-input bg-background px-2 text-sm ring-offset-background focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <option value="owner">Owner</option>
                    <option value="admin">Admin</option>
                    <option value="developer">Developer</option>
                    <option value="viewer">Viewer</option>
                  </select>
                  <Button
                    variant="destructive"
                    size="sm"
                    onclick={() => onRemoveMember(member)}
                    disabled={busy.has(`remove-${member.user_id}`)}
                    data-testid={`remove-${member.user_id}`}
                  >
                    Remove
                  </Button>
                </div>
              </TableCell>
            </TableRow>
          {/each}
        </TableBody>
      </Table>
    {/if}
  </CardContent>
</Card>

<Separator class="my-6" />

<!-- Section 2: Invite -->
<Card>
  <CardHeader>
    <CardTitle>Invite member</CardTitle>
    <CardDescription>Send an invite link to a new team member.</CardDescription>
  </CardHeader>
  <CardContent class="space-y-6">
    <form onsubmit={onCreateInvite} class="flex flex-col gap-3 sm:flex-row sm:items-end">
      <div class="flex-1 space-y-1.5">
        <Label for="invite-email">Email address</Label>
        <Input
          id="invite-email"
          type="email"
          bind:value={inviteEmail}
          autocomplete="off"
          placeholder="teammate@example.com"
          data-testid="invite-email"
          required
        />
      </div>
      <div class="space-y-1.5">
        <Label for="invite-role">Role</Label>
        <select
          id="invite-role"
          bind:value={inviteRole}
          data-testid="invite-role"
          class="h-10 rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2"
        >
          <option value="admin">Admin</option>
          <option value="developer">Developer</option>
          <option value="viewer">Viewer</option>
        </select>
      </div>
      <Button type="submit" disabled={inviteSubmitting} data-testid="invite-submit">
        {inviteSubmitting ? 'Issuing…' : 'Send invite'}
      </Button>
    </form>

    {#if inviteFormError}
      <div class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive" role="alert">
        {inviteFormError}
      </div>
    {/if}

    {#if lastIssuedToken}
      <div class="rounded-md border border-amber-300 bg-amber-50 dark:border-amber-700 dark:bg-amber-950/40 p-4" role="alert" data-testid="issued-token">
        <p class="text-sm font-medium text-amber-800 dark:text-amber-300 mb-2">
          Copy and share this invite link — it will not be shown again.
        </p>
        <code class="block rounded border border-amber-200 dark:border-amber-800 bg-white dark:bg-zinc-900 px-3 py-2 font-mono text-sm break-all select-all text-zinc-900 dark:text-zinc-100">
          {lastIssuedToken}
        </code>
      </div>
    {/if}

    <!-- Pending invites subsection -->
    {#if invites && invites.length > 0}
      <div>
        <h3 class="text-sm font-semibold mb-3 text-foreground">Pending invites</h3>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Email</TableHead>
              <TableHead>Role</TableHead>
              <TableHead>Expires</TableHead>
              <TableHead class="w-[100px]">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {#each invites as invite (invite.id)}
              <TableRow>
                <TableCell class="text-sm">{invite.email}</TableCell>
                <TableCell>
                  <Badge variant={roleBadgeVariant(invite.role)}>{invite.role}</Badge>
                </TableCell>
                <TableCell class="text-sm text-muted-foreground">
                  {new Date(invite.expires_at).toLocaleString()}
                </TableCell>
                <TableCell>
                  {#if inviteStatus(invite) === 'pending'}
                    <Button
                      variant="destructive"
                      size="sm"
                      onclick={() => onRevokeInvite(invite)}
                      disabled={busy.has(`invite-${invite.id}`)}
                      data-testid={`revoke-${invite.id}`}
                    >
                      Revoke
                    </Button>
                  {:else}
                    <span class="text-xs text-muted-foreground capitalize">{inviteStatus(invite)}</span>
                  {/if}
                </TableCell>
              </TableRow>
            {/each}
          </TableBody>
        </Table>
      </div>
    {/if}
  </CardContent>
</Card>
