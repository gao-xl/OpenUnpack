namespace ARchivePasswordFinder.CrackEngine.Interfaces;

public interface IMovingAverage
{
	double Average { get; }

	void AddSample(double val);

	void ClearSamples();

	void InitializeSamples(double val);
}
