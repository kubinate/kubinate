// See https://svelte.dev/docs/kit/types#app
declare global {
  namespace App {
    interface Error {
      code?: string;
      id?: string;
    }
    interface Locals {
      session?: {
        userId: string;
        organizationId?: string;
      };
    }
  }
}

export {};
