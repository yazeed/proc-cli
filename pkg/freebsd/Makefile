PORTNAME=	proc-cli
DISTVERSIONPREFIX=	v
DISTVERSION=	2.3.0
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
		share/man/man1/proc.1.gz \
		share/zsh/site-functions/_proc

post-install:
	${STAGEDIR}${PREFIX}/bin/proc manpage > ${WRKDIR}/proc.1
	${INSTALL_MAN} ${WRKDIR}/proc.1 ${STAGEDIR}${PREFIX}/share/man/man1
	@${MKDIR} ${STAGEDIR}${PREFIX}/etc/bash_completion.d/
	${STAGEDIR}${PREFIX}/bin/proc completions bash > ${STAGEDIR}${PREFIX}/etc/bash_completion.d/proc.bash
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/fish/vendor_completions.d/
	${STAGEDIR}${PREFIX}/bin/proc completions fish > ${STAGEDIR}${PREFIX}/share/fish/vendor_completions.d/proc.fish
	@${MKDIR} ${STAGEDIR}${PREFIX}/share/zsh/site-functions/
	${STAGEDIR}${PREFIX}/bin/proc completions zsh > ${STAGEDIR}${PREFIX}/share/zsh/site-functions/_proc

.include <bsd.port.mk>
