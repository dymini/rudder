import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { useState } from "react";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { AuthProvider } from "../lib/auth";
import { colors } from "../lib/theme";

export default function RootLayout() {
  const [queryClient] = useState(
    () =>
      new QueryClient({
        defaultOptions: {
          queries: { retry: 1, staleTime: 15_000 },
        },
      }),
  );
  return (
    <QueryClientProvider client={queryClient}>
      <AuthProvider>
        <StatusBar style="light" />
        <ErrorBoundary>
          <Stack
            screenOptions={{
              headerStyle: { backgroundColor: colors.bg },
              headerTintColor: colors.text,
              headerTitleStyle: { fontWeight: "600", fontSize: 17 },
              headerShadowVisible: false,
              contentStyle: { backgroundColor: colors.bg },
            }}
          >
            <Stack.Screen name="index" options={{ headerShown: false }} />
            <Stack.Screen name="connect" options={{ headerShown: false }} />
            <Stack.Screen name="(tabs)" options={{ headerShown: false }} />
          </Stack>
        </ErrorBoundary>
      </AuthProvider>
    </QueryClientProvider>
  );
}
