using System.Runtime.InteropServices;

namespace MultiLanguage;

[ComImport]
[CoClass(typeof(CMultiLanguageClass))]
[Guid("275C23E1-3747-11D0-9FEA-00AA003F8646")]
public interface CMultiLanguage : IMultiLanguage
{
}
