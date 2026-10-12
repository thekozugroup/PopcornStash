/**
 * Bridges Astryx links (SideNavItem, Tab, Button href, Link) to React Router
 * so in-app navigation stays client-side. Registered once through
 * `<LinkProvider component={RouterLink}>`.
 */
import type {AnchorHTMLAttributes, Ref} from 'react';
import {Link} from 'react-router';

type RouterLinkProps = AnchorHTMLAttributes<HTMLAnchorElement> & {
  ref?: Ref<HTMLAnchorElement>;
};

function isExternal(href: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith('//');
}

export function RouterLink({href, ref, target, download, ...rest}: RouterLinkProps) {
  // API URLs (files, downloads) and external links are real page loads.
  if (!href || isExternal(href) || href.startsWith('/api/') || target === '_blank' || download !== undefined) {
    return <a ref={ref} href={href} target={target} download={download} {...rest} />;
  }
  return <Link ref={ref} to={href} target={target} {...rest} />;
}
