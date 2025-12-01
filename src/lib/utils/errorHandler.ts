import { toast } from "svelte-sonner";
import logger, { type LogContext } from "./logger";

export interface ErrorDisplayOptions {
  showToast?: boolean;
  toastDuration?: number;
  userMessage?: string;
  logContext?: LogContext;
  silent?: boolean; // if true, do not show toast
}

/**
 * Centralized error handler that logs structured errors and optionally shows user-friendly messages
 */
export function handleError(
  error: Error | unknown,
  component: string,
  operation: string,
  options: ErrorDisplayOptions = {}
) {
  const {
    showToast = true,
    toastDuration = 5000,
    userMessage,
    logContext = {},
    silent = false
  } = options;

  const err = error instanceof Error ? error : new Error(String(error));
  
  // Log the error with context
  logger.error(`${operation} failed`, {
    component,
    operation,
    ...logContext
  }, err);

  // Show user-friendly toast if requested and not silent
  if (showToast && !silent) {
    const displayMessage = userMessage || getDefaultUserMessage(operation, err);
    try {
      toast.error(displayMessage, {
        duration: toastDuration,
        description: import.meta.env.DEV ? err.message : undefined
      });
    } catch (e) {
      // toast may not be available in some environments (eg. tests)
      // eslint-disable-next-line no-console
      console.warn('Toast unavailable', e);
    }
  }

  return err;
}

/**
 * Generate user-friendly error messages based on operation type
 */
function getDefaultUserMessage(operation: string, error: Error): string {
  const message = (error.message || '').toLowerCase();
  
  // Network-related errors
  if (message.includes('network') || message.includes('connection') || message.includes('timeout')) {
    return "Network connection issue. Please check your internet connection and try again.";
  }
  
  // File system errors
  if (message.includes('permission') || message.includes('access')) {
    return "Permission denied. Please check file access permissions.";
  }
  
  if (message.includes('space') || message.includes('disk')) {
    return "Insufficient disk space. Please free up space and try again.";
  }
  
  // DHT/P2P errors
  if (operation.toLowerCase().includes('dht') || operation.toLowerCase().includes('peer')) {
    return "Peer network issue. Please try again in a moment.";
  }
  
  // Upload/download errors
  if (operation.toLowerCase().includes('upload')) {
    return "Upload failed. Please check your file and try again.";
  }
  
  if (operation.toLowerCase().includes('download')) {
    return "Download failed. The file may no longer be available.";
  }
  
  // Wallet/payment errors
  if (operation.toLowerCase().includes('wallet') || operation.toLowerCase().includes('payment')) {
    return "Wallet operation failed. Please check your account and try again.";
  }
  
  // Generic fallback
  return "An unexpected error occurred. Please try again.";
}

/**
 * Higher-order function to wrap async operations with error handling
 */
export function withErrorHandling<T extends any[], R>(
  fn: (...args: T) => Promise<R>,
  component: string,
  operation: string,
  options?: ErrorDisplayOptions
) {
  return async (...args: T): Promise<R | null> => {
    try {
      return await fn(...args);
    } catch (error) {
      handleError(error, component, operation, options);
      return null;
    }
  };
}

/**
 * Retry wrapper with exponential backoff
 */
export async function withRetry<T>(
  fn: () => Promise<T>,
  component: string,
  operation: string,
  maxRetries = 3,
  baseDelay = 1000
): Promise<T> {
  let lastError: Error;
  
  for (let attempt = 1; attempt <= maxRetries; attempt++) {
    try {
      return await fn();
    } catch (error) {
      lastError = error instanceof Error ? error : new Error(String(error));
      
      if (attempt === maxRetries) {
        handleError(lastError, component, operation, {
          userMessage: `${operation} failed after ${maxRetries} attempts. Please try again later.`,
          logContext: { attempts: maxRetries }
        });
        throw lastError;
      }
      
      // Log retry attempt
      logger.warn(`${operation} failed, retrying in ${baseDelay * attempt}ms`, {
        component,
        operation,
        attempt,
        maxRetries
      }, lastError);
      
      // Exponential backoff
      await new Promise(resolve => setTimeout(resolve, baseDelay * attempt));
    }
  }
  
  throw lastError!;
}

export default {
  handleError,
  withErrorHandling,
  withRetry
};
