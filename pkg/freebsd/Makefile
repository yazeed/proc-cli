PORTNAME=	proc-cli
DISTVERSIONPREFIX=	v
DISTVERSION=	2.4.0
CATEGORIES=	sysutils

MAINTAINER=	me@yazeed.com
COMMENT=	Semantic CLI tool for process management
WWW=		https://proc-cli.com/

LICENSE=	MIT
LICENSE_FILE=	${WRKSRC}/LICENSE

USES=		cargo
USE_GITHUB=	yes
GH_ACCOUNT=	yazeed

PLIST_FILES=	bin/proc \
		etc/bash_completion.d/proc.bash \
		share/fish/vendor_completions.d/proc.fish \
		share/man/man1/proc-by.1.gz \
		share/man/man1/proc-for.1.gz \
		share/man/man1/proc-free.1.gz \
		share/man/man1/proc-freeze.1.gz \
		share/man/man1/proc-in.1.gz \
		share/man/man1/proc-info.1.gz \
		share/man/man1/proc-kill.1.gz \
		share/man/man1/proc-list.1.gz \
		share/man/man1/proc-on.1.gz \
		share/man/man1/proc-orphans.1.gz \
		share/man/man1/proc-ports.1.gz \
		share/man/man1/proc-stop.1.gz \
		share/man/man1/proc-stuck.1.gz \
		share/man/man1/proc-thaw.1.gz \
		share/man/man1/proc-tree.1.gz \
		share/man/man1/proc-unstick.1.gz \
		share/man/man1/proc-wait.1.gz \
		share/man/man1/proc-watch.1.gz \
		share/man/man1/proc-why.1.gz \
		share/man/man1/proc.1.gz \
		share/zsh/site-functions/_proc

post-install:
	${STAGEDIR}${PREFIX}/bin/proc manpage --dir ${WRKDIR}/man
	${INSTALL_MAN} ${WRKDIR}/man/*.1 ${STAGEDIR}${PREFIX}/share/man/man1
	@${MKDIR} ${STAGEDIR}${PREFIX}/etc/bash_completion.d/
	${STAGEDIR}${PREFIX}/bin/proc completions bash > ${STAGEDIR}${PREFIX}/etc/bash_completion.d/proc.bash
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/fish/vendor_completions.d/
	${STAGEDIR}${PREFIX}/bin/proc completions fish > ${STAGEDIR}${PREFIX}/share/fish/vendor_completions.d/proc.fish
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/zsh/site-functions/
	${STAGEDIR}${PREFIX}/bin/proc completions zsh > ${STAGEDIR}${PREFIX}/share/zsh/site-functions/_proc

.include <bsd.port.mk>
