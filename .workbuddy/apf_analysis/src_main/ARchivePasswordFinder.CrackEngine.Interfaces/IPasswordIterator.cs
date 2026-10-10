namespace ARchivePasswordFinder.CrackEngine.Interfaces;

public interface IPasswordIterator
{
	CharsetCasing PasswordCasing { get; set; }

	bool RemoveWhitespace { get; set; }

	bool Initialize(CharsetEncoding passwordEncoding, int maxPasswordLength, ref string errorMsg);

	string GetNextPassword();
}
