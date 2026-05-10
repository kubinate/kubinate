<script lang="ts">
  import { page } from '$app/state';
  import MfaBanner from '$lib/components/MfaBanner.svelte';
  import { Avatar, AvatarFallback } from '$lib/components/ui/avatar';
  import {
    Sidebar,
    SidebarContent,
    SidebarFooter,
    SidebarGroup,
    SidebarGroupContent,
    SidebarGroupLabel,
    SidebarHeader,
    SidebarInset,
    SidebarMenu,
    SidebarMenuButton,
    SidebarMenuItem,
    SidebarProvider,
    SidebarRail,
    SidebarTrigger
  } from '$lib/components/ui/sidebar';
  import { Badge } from '$lib/components/ui/badge';
  import {
    Building2,
    CreditCard,
    FileText,
    KeyRound,
    LayoutGrid,
    LogOut,
    Puzzle,
    Shield,
    User,
    Users
  } from 'lucide-svelte';
  import type { MfaState } from '$lib/api/schemas';

  interface Props {
    data: {
      session: { userId: string; organizationId: string; displayName?: string; email?: string };
      mfa_state: MfaState | null;
    };
    children: import('svelte').Snippet;
  }

  let { data, children }: Props = $props();

  const navPlatform = [
    { label: 'Clusters', href: '/app', icon: LayoutGrid },
    { label: 'Add-ons', href: '/app/addons', icon: Puzzle }
  ];

  const navSettings = [
    { label: 'Profile', href: '/app/settings/profile', icon: User },
    { label: 'General', href: '/app/settings/general', icon: Building2 },
    { label: 'Team', href: '/app/settings/team', icon: Users },
    { label: 'Billing', href: '/app/settings/billing', icon: CreditCard },
    { label: 'Security', href: '/app/settings/security', icon: Shield },
    { label: 'Integrations', href: '/app/settings/integrations', icon: KeyRound },
    { label: 'Audit log', href: '/app/settings/audit-log', icon: FileText }
  ];

  const userLabel = $derived(data.session.displayName || data.session.email || data.session.userId);

  let userInitial = $derived(userLabel.charAt(0).toUpperCase());

  function isActive(href: string): boolean {
    const pathname = page.url.pathname;
    if (href === '/app') {
      return pathname === '/app' || pathname === '/app/';
    }
    return pathname.startsWith(href);
  }
</script>

<SidebarProvider>
  <Sidebar>
    <!-- Wordmark + badge -->
    <SidebarHeader class="px-4 py-4">
      <div class="flex items-center gap-2">
        <span class="text-lg font-bold tracking-tight">Kubinate</span>
        <Badge variant="secondary" class="text-xs">k3s</Badge>
      </div>
    </SidebarHeader>

    <SidebarContent>
      <!-- Platform group -->
      <SidebarGroup>
        <SidebarGroupLabel>Platform</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {#each navPlatform as item (item.href)}
              <SidebarMenuItem>
                <SidebarMenuButton isActive={isActive(item.href)}>
                  {#snippet child({ props })}
                    <a href={item.href} {...props}>
                      <item.icon class="size-4" />
                      <span>{item.label}</span>
                    </a>
                  {/snippet}
                </SidebarMenuButton>
              </SidebarMenuItem>
            {/each}
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>

      <!-- Settings group -->
      <SidebarGroup>
        <SidebarGroupLabel>Settings</SidebarGroupLabel>
        <SidebarGroupContent>
          <SidebarMenu>
            {#each navSettings as item (item.href)}
              <SidebarMenuItem>
                <SidebarMenuButton isActive={isActive(item.href)}>
                  {#snippet child({ props })}
                    <a href={item.href} {...props}>
                      <item.icon class="size-4" />
                      <span>{item.label}</span>
                    </a>
                  {/snippet}
                </SidebarMenuButton>
              </SidebarMenuItem>
            {/each}
          </SidebarMenu>
        </SidebarGroupContent>
      </SidebarGroup>
    </SidebarContent>

    <!-- User footer -->
    <SidebarFooter class="px-4 py-3">
      <div class="flex items-center gap-3">
        <Avatar class="size-8 shrink-0">
          <AvatarFallback class="text-xs font-medium">{userInitial}</AvatarFallback>
        </Avatar>
        <span class="text-muted-foreground min-w-0 flex-1 truncate text-xs" title={userLabel}>
          {userLabel}
        </span>
        <a
          href="/api/v1/auth/logout"
          aria-label="Sign out"
          class="text-muted-foreground hover:text-foreground shrink-0 transition-colors"
        >
          <LogOut class="size-4" />
        </a>
      </div>
    </SidebarFooter>

    <SidebarRail />
  </Sidebar>

  <SidebarInset>
    <!-- Topbar -->
    <header class="border-border flex h-12 shrink-0 items-center gap-2 border-b px-4">
      <SidebarTrigger class="-ml-1" />
    </header>

    <!-- MFA banner + page content -->
    <div class="flex flex-1 flex-col">
      <MfaBanner mfaState={data.mfa_state} />
      <main class="flex-1 p-6">
        {@render children()}
      </main>
    </div>
  </SidebarInset>
</SidebarProvider>
