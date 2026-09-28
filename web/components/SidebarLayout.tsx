import React from 'react';

interface SidebarLayoutProps {
  topContent?: React.ReactNode;
  sidebar: React.ReactNode;
  main: React.ReactNode;
}

export function SidebarLayout({ topContent, sidebar, main }: SidebarLayoutProps) {
  return (
    <>
      {topContent && (
        <div className="mb-6">
          {topContent}
        </div>
      )}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="space-y-4">
          {sidebar}
        </div>
        <div>
          {main}
        </div>
      </div>
    </>
  );
}
