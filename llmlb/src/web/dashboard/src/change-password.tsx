import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Toaster } from '@/components/ui/toaster'
import ChangePasswordPage from './pages/ChangePassword'
import './index.css'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <TooltipProvider>
      <ChangePasswordPage />
      <Toaster />
    </TooltipProvider>
  </StrictMode>
)
